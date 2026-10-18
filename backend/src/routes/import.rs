use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiJson, ApiResult},
    format::{DiaryEntry, DiaryPayload, DiaryTag},
    routes::export::Scope,
    state::AppState,
    util::{now_unix, parse_rfc3339, valid_entry_date, valid_filter_date},
};

#[derive(Deserialize)]
pub struct PreviewRequest {
    pub entries: Vec<DiaryEntry>,
    #[serde(default)]
    pub tags: Vec<DiaryTag>,
    pub scope: Scope,
    #[serde(default)]
    pub dates: Vec<String>,
}

#[derive(Deserialize)]
pub struct CommitRequest {
    pub import_id: String,
    pub scope: Scope,
    #[serde(default)]
    pub dates: Vec<String>,
}

#[derive(Clone)]
struct NormalizedTag {
    name: String,
    color: Option<String>,
    emoji: Option<String>,
}

struct PreparedEntry {
    date: String,
    content_md: String,
    tags: Vec<NormalizedTag>,
    created_at: Option<i64>,
}

struct Prepared {
    entries: Vec<PreparedEntry>,
    declared: HashMap<String, NormalizedTag>,
    warnings: Vec<String>,
}

#[derive(FromRow)]
struct LocalTag {
    id: i64,
    name: String,
    deleted_at: Option<i64>,
}

pub async fn preview(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<PreviewRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let payload = DiaryPayload {
        format: "diary".into(),
        version: 1,
        exported_at: String::new(),
        app: String::new(),
        timezone: "UTC".into(),
        tags: request.tags,
        entries: request.entries,
    };

    let encoded_len = serde_json::to_vec(&payload)
        .map_err(ApiError::internal)?
        .len();
    if encoded_len > state.config.max_import_bytes {
        return Err(ApiError::too_large("import_too_large", "导入内容超过上限"));
    }

    let prepared = prepare_payload(&payload, &state)?;
    let selected = select_entries(&prepared.entries, request.scope, &request.dates)?;

    let local_tags = load_local_tags(&state, user.id).await?;
    let local_tag_map: HashMap<String, &LocalTag> = local_tags
        .iter()
        .map(|tag| (name_norm(&tag.name), tag))
        .collect();

    let selected_dates: Vec<String> = selected.iter().map(|entry| entry.date.clone()).collect();
    let existing_dates = load_existing_dates(&state, user.id, &selected_dates).await?;

    let mut create_dates = Vec::new();
    let mut update_dates = Vec::new();
    for date in &selected_dates {
        if existing_dates.contains(date) {
            update_dates.push(date.clone());
        } else {
            create_dates.push(date.clone());
        }
    }

    let referenced = referenced_tags(&selected);
    let mut tags_to_create = Vec::new();
    let mut missing_count = 0usize;
    for referenced_tag in &referenced {
        let norm = name_norm(&referenced_tag.name);
        match local_tag_map.get(&norm) {
            Some(tag) if tag.deleted_at.is_none() => {}
            Some(_) => {}
            None => {
                missing_count += 1;
                let tag = prepared
                    .declared
                    .get(&norm)
                    .cloned()
                    .unwrap_or_else(|| referenced_tag.clone());
                tags_to_create.push(DiaryTag {
                    name: tag.name,
                    color: tag.color,
                    emoji: tag.emoji,
                });
            }
        }
    }
    tags_to_create.sort_by(|a, b| a.name.cmp(&b.name));

    if local_tags.len() + missing_count > state.config.max_tags as usize {
        return Err(ApiError::conflict(
            "tag_limit",
            format!("导入后将超过 {} 个标签上限", state.config.max_tags),
        ));
    }

    let import_id = Uuid::new_v4().to_string();
    state.imports.put(user.id, import_id.clone(), payload).await;

    let mut warnings = prepared.warnings;
    warnings.sort();
    warnings.dedup();

    Ok(Json(json!({
        "import_id": import_id,
        "summary": {
            "create": create_dates.len(),
            "update": update_dates.len(),
            "tags_create": tags_to_create.len(),
        },
        "tags_to_create": tags_to_create,
        "dates": { "create": create_dates, "update": update_dates },
        "warnings": warnings,
    })))
}

pub async fn commit(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<CommitRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some(payload) = state.imports.get(user.id, &request.import_id).await else {
        return Err(ApiError::not_found(
            "import_expired",
            "导入预检已过期，请重新预检",
        ));
    };

    let prepared = prepare_payload(&payload, &state)?;
    let selected = select_entries(&prepared.entries, request.scope, &request.dates)?;

    let mut tx = state.pool.begin().await?;
    let local_tags =
        sqlx::query_as::<_, LocalTag>("SELECT id, name, deleted_at FROM tags WHERE user_id = ?")
            .bind(user.id)
            .fetch_all(&mut *tx)
            .await?;
    let mut tag_ids: HashMap<String, i64> = local_tags
        .iter()
        .map(|tag| (name_norm(&tag.name), tag.id))
        .collect();
    let mut archived: HashSet<String> = local_tags
        .iter()
        .filter(|tag| tag.deleted_at.is_some())
        .map(|tag| name_norm(&tag.name))
        .collect();

    let now = now_unix();
    let referenced = referenced_tags(&selected);
    let mut tags_created = 0i64;

    for referenced_tag in &referenced {
        let norm = name_norm(&referenced_tag.name);
        if let Some(id) = tag_ids.get(&norm).copied() {
            if archived.remove(&norm) {
                sqlx::query("UPDATE tags SET deleted_at = NULL WHERE id = ? AND user_id = ?")
                    .bind(id)
                    .bind(user.id)
                    .execute(&mut *tx)
                    .await?;
            }
            continue;
        }

        let tag = prepared
            .declared
            .get(&norm)
            .cloned()
            .unwrap_or_else(|| referenced_tag.clone());

        let inserted = sqlx::query(
            "INSERT INTO tags (user_id, name, name_norm, color, emoji, sort_order, created_at) \
             VALUES (?, ?, ?, ?, ?, 0, ?) \
             ON CONFLICT (user_id, name_norm) DO NOTHING",
        )
        .bind(user.id)
        .bind(&tag.name)
        .bind(&norm)
        .bind(&tag.color)
        .bind(&tag.emoji)
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let row: (i64, Option<i64>) =
            sqlx::query_as("SELECT id, deleted_at FROM tags WHERE user_id = ? AND name_norm = ?")
                .bind(user.id)
                .bind(&norm)
                .fetch_one(&mut *tx)
                .await?;

        if row.1.is_some() {
            sqlx::query("UPDATE tags SET deleted_at = NULL WHERE id = ?")
                .bind(row.0)
                .execute(&mut *tx)
                .await?;
        }
        if inserted > 0 {
            tags_created += 1;
        }
        tag_ids.insert(norm, row.0);
    }

    let mut created = 0i64;
    let mut updated = 0i64;

    for entry in selected {
        let tags: Vec<i64> = entry
            .tags
            .iter()
            .filter_map(|tag| tag_ids.get(&name_norm(&tag.name)).copied())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM entries WHERE user_id = ? AND entry_date = ?")
                .bind(user.id)
                .bind(&entry.date)
                .fetch_optional(&mut *tx)
                .await?;

        let entry_id = if let Some(id) = existing {
            sqlx::query(
                "UPDATE entries SET content_md = ?, updated_at = ?, imported_at = ? WHERE id = ?",
            )
            .bind(&entry.content_md)
            .bind(now)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            updated += 1;
            id
        } else {
            let created_at = entry.created_at.unwrap_or(now);
            let result = sqlx::query(
                "INSERT INTO entries \
                 (user_id, entry_date, content_md, created_at, updated_at, imported_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(user.id)
            .bind(&entry.date)
            .bind(&entry.content_md)
            .bind(created_at)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            created += 1;
            result.last_insert_rowid()
        };

        sqlx::query("DELETE FROM entry_tags WHERE entry_id = ?")
            .bind(entry_id)
            .execute(&mut *tx)
            .await?;
        for tag_id in tags {
            sqlx::query("INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?, ?)")
                .bind(entry_id)
                .bind(tag_id)
                .execute(&mut *tx)
                .await?;
        }
    }

    tx.commit().await?;
    state.imports.remove(user.id).await;

    Ok(Json(json!({
        "created": created,
        "updated": updated,
        "tags_created": tags_created,
    })))
}

fn prepare_payload(payload: &DiaryPayload, state: &AppState) -> ApiResult<Prepared> {
    if payload.entries.len() > 100_000 {
        return Err(ApiError::bad_request(
            "bad_file_format",
            "日记数量超过 100000 条",
        ));
    }
    if payload.tags.len() > 500 {
        return Err(ApiError::bad_request(
            "bad_file_format",
            "标签数量超过 500 个",
        ));
    }

    let mut warnings = Vec::new();
    let mut declared: HashMap<String, NormalizedTag> = HashMap::new();
    for tag in &payload.tags {
        let normalized = normalize_tag(&tag.name, tag.color.as_deref(), tag.emoji.as_deref())
            .ok_or_else(|| {
                ApiError::bad_request("bad_file_format", format!("标签名非法: {}", tag.name))
            })?;
        let norm = name_norm(&normalized.name);
        if declared.insert(norm.clone(), normalized).is_some() {
            warnings.push(format!("标签 {norm} 重复定义，已使用最后一个"));
        }
    }

    let mut entries = Vec::with_capacity(payload.entries.len());
    let mut seen_dates = HashSet::new();
    let mut unknown_tags = HashSet::new();

    for entry in &payload.entries {
        if !valid_entry_date(&entry.date) {
            return Err(ApiError::bad_request(
                "bad_file_format",
                format!("日记日期非法: {}", entry.date),
            ));
        }
        if !seen_dates.insert(entry.date.clone()) {
            return Err(ApiError::bad_request(
                "bad_file_format",
                format!("日记日期重复: {}", entry.date),
            ));
        }
        if entry.content_md.len() > state.config.max_entry_bytes {
            return Err(ApiError::too_large(
                "entry_too_large",
                format!("{} 的正文超过上限", entry.date),
            ));
        }
        if entry.tags.len() > state.config.max_tags_per_entry {
            return Err(ApiError::bad_request(
                "bad_file_format",
                format!(
                    "{} 的标签超过 {} 个",
                    entry.date, state.config.max_tags_per_entry
                ),
            ));
        }
        let created_at = match entry.created_at.as_deref() {
            Some(value) => Some(parse_rfc3339(value).ok_or_else(|| {
                ApiError::bad_request(
                    "bad_file_format",
                    format!("{} 的 created_at 不是 RFC3339", entry.date),
                )
            })?),
            None => None,
        };

        let mut tags = Vec::new();
        let mut seen_tags = HashSet::new();
        for name in &entry.tags {
            let normalized = match declared.get(&name_norm(name)) {
                Some(tag) => tag.clone(),
                None => {
                    let normalized = normalize_tag(name, None, None).ok_or_else(|| {
                        ApiError::bad_request(
                            "bad_file_format",
                            format!("{} 引用了非法标签名", entry.date),
                        )
                    })?;
                    unknown_tags.insert(name_norm(name));
                    normalized
                }
            };
            let norm = name_norm(&normalized.name);
            if seen_tags.insert(norm) {
                tags.push(normalized);
            }
        }

        entries.push(PreparedEntry {
            date: entry.date.clone(),
            content_md: entry.content_md.clone(),
            tags,
            created_at,
        });
    }

    for norm in unknown_tags {
        warnings.push(format!("标签 {norm} 未在文件中声明，已自动创建"));
    }

    entries.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(Prepared {
        entries,
        declared,
        warnings,
    })
}

fn select_entries<'a>(
    entries: &'a [PreparedEntry],
    scope: Scope,
    dates: &[String],
) -> ApiResult<Vec<&'a PreparedEntry>> {
    if matches!(scope, Scope::All) {
        return Ok(entries.iter().collect());
    }
    if dates.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_dates",
            "scope=dates 时 dates 不能为空",
        ));
    }
    let mut selected_dates = HashSet::new();
    for date in dates {
        if !valid_filter_date(date) {
            return Err(ApiError::bad_request(
                "invalid_date",
                format!("非法日期: {date}"),
            ));
        }
        selected_dates.insert(date.clone());
    }
    Ok(entries
        .iter()
        .filter(|entry| selected_dates.contains(&entry.date))
        .collect())
}

fn referenced_tags(entries: &[&PreparedEntry]) -> Vec<NormalizedTag> {
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for entry in entries {
        for tag in &entry.tags {
            let norm = name_norm(&tag.name);
            if seen.insert(norm) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

async fn load_local_tags(state: &AppState, user_id: i64) -> ApiResult<Vec<LocalTag>> {
    Ok(
        sqlx::query_as::<_, LocalTag>("SELECT id, name, deleted_at FROM tags WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(&state.pool)
            .await?,
    )
}

async fn load_existing_dates(
    state: &AppState,
    user_id: i64,
    dates: &[String],
) -> ApiResult<HashSet<String>> {
    if dates.is_empty() {
        return Ok(HashSet::new());
    }
    let mut builder =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT entry_date FROM entries WHERE user_id = ");
    builder.push_bind(user_id);
    builder.push(" AND entry_date IN (");
    let mut separated = builder.separated(", ");
    for date in dates {
        separated.push_bind(date);
    }
    builder.push(")");
    let rows: Vec<String> = builder.build_query_scalar().fetch_all(&state.pool).await?;
    Ok(rows.into_iter().collect())
}

fn normalize_tag(name: &str, color: Option<&str>, emoji: Option<&str>) -> Option<NormalizedTag> {
    let name = name.trim();
    let len = name.chars().count();
    if !(1..=32).contains(&len) {
        return None;
    }
    Some(NormalizedTag {
        name: name.to_owned(),
        color: color.and_then(normalize_color),
        emoji: emoji.and_then(normalize_emoji),
    })
}

fn normalize_color(color: &str) -> Option<String> {
    let color = color.trim().to_ascii_lowercase();
    (color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(color)
}

fn normalize_emoji(emoji: &str) -> Option<String> {
    let emoji = emoji.trim();
    (!emoji.is_empty()
        && emoji.len() <= 8
        && UnicodeSegmentation::graphemes(emoji, true).count() == 1)
        .then(|| emoji.to_owned())
}

fn name_norm(name: &str) -> String {
    name.trim().to_lowercase()
}
