use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiJson, ApiResult},
    state::AppState,
    util::{now_unix, parse_rfc3339, to_rfc3339, valid_entry_date, valid_filter_date},
};

#[derive(Debug, FromRow)]
struct EntryRow {
    id: i64,
    entry_date: String,
    content_md: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, FromRow)]
struct TagBriefRow {
    entry_id: i64,
    id: i64,
    name: String,
    color: Option<String>,
    emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagBrief {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub emoji: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EntryResponse {
    pub date: String,
    pub content_md: String,
    pub tags: Vec<TagBrief>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Deserialize)]
pub struct UpsertRequest {
    pub content_md: String,
    #[serde(default)]
    pub tag_ids: Vec<i64>,
    pub base_updated_at: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<EntryResponse>>> {
    let from = query.from.as_deref().map(validate_query_date).transpose()?;
    let to = query.to.as_deref().map(validate_query_date).transpose()?;
    if matches!((&from, &to), (Some(from), Some(to)) if from > to) {
        return Err(ApiError::bad_request(
            "invalid_date_range",
            "from 不能晚于 to",
        ));
    }

    let rows = sqlx::query_as::<_, EntryRow>(
        "SELECT id, entry_date, content_md, created_at, updated_at FROM entries \
         WHERE user_id = ? \
           AND (? IS NULL OR entry_date >= ?) \
           AND (? IS NULL OR entry_date <= ?) \
         ORDER BY entry_date",
    )
    .bind(user.id)
    .bind(&from)
    .bind(&from)
    .bind(&to)
    .bind(&to)
    .fetch_all(&state.pool)
    .await?;

    let tags = load_tags_for_entries(&state.pool, user.id, &rows).await?;
    let entries = rows
        .into_iter()
        .map(|row| {
            let entry_tags = tags
                .iter()
                .filter(|tag| tag.entry_id == row.id)
                .map(|tag| TagBrief {
                    id: tag.id,
                    name: tag.name.clone(),
                    color: tag.color.clone(),
                    emoji: tag.emoji.clone(),
                })
                .collect();
            EntryResponse {
                date: row.entry_date,
                content_md: row.content_md,
                tags: entry_tags,
                created_at: to_rfc3339(row.created_at),
                updated_at: to_rfc3339(row.updated_at),
            }
        })
        .collect();

    Ok(Json(entries))
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(date): Path<String>,
) -> ApiResult<Json<EntryResponse>> {
    validate_path_date(&date)?;
    let entry = fetch_entry(&state.pool, user.id, &date)
        .await?
        .ok_or_else(|| ApiError::not_found("entry_not_found", "该日期还没有日记"))?;
    Ok(Json(entry))
}

pub async fn upsert(
    State(state): State<AppState>,
    user: AuthUser,
    Path(date): Path<String>,
    ApiJson(request): ApiJson<UpsertRequest>,
) -> ApiResult<Response> {
    validate_path_date(&date)?;
    let content_len = request.content_md.len();
    if content_len > state.config.max_entry_bytes {
        return Err(ApiError::too_large(
            "entry_too_large",
            format!("正文最多 {} 字节", state.config.max_entry_bytes),
        ));
    }
    if request.tag_ids.len() > state.config.max_tags_per_entry {
        return Err(ApiError::bad_request(
            "too_many_tags",
            format!("每篇日记最多 {} 个标签", state.config.max_tags_per_entry),
        ));
    }

    let base_updated_at = request
        .base_updated_at
        .as_deref()
        .map(|value| {
            parse_rfc3339(value).ok_or_else(|| {
                ApiError::bad_request("invalid_timestamp", "base_updated_at 不是 RFC3339 时间")
            })
        })
        .transpose()?;

    let tag_ids: Vec<i64> = request
        .tag_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;

    let existing = sqlx::query_as::<_, EntryRow>(
        "SELECT id, entry_date, content_md, created_at, updated_at FROM entries \
         WHERE user_id = ? AND entry_date = ?",
    )
    .bind(user.id)
    .bind(&date)
    .fetch_optional(&mut *tx)
    .await?;

    if let (Some(base), Some(current)) = (base_updated_at, &existing)
        && current.updated_at != base
    {
        tx.rollback().await?;
        let current = fetch_entry(&state.pool, user.id, &date)
            .await?
            .map(|entry| serde_json::to_value(entry).unwrap_or_default());
        return Err(ApiError::conflict("stale_write", "另一处已修改该日记")
            .with_details(current.unwrap_or_default()));
    }

    if !tag_ids.is_empty() {
        let mut allowed_tags: HashSet<i64> =
            sqlx::query_scalar("SELECT id FROM tags WHERE user_id = ? AND deleted_at IS NULL")
                .bind(user.id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();

        // Archived tags may stay attached to an existing entry; they just cannot
        // be newly selected. Otherwise editing an old diary would silently break.
        if let Some(current) = &existing {
            let existing_tags: Vec<i64> =
                sqlx::query_scalar("SELECT tag_id FROM entry_tags WHERE entry_id = ?")
                    .bind(current.id)
                    .fetch_all(&mut *tx)
                    .await?;
            allowed_tags.extend(existing_tags);
        }

        if let Some(unknown) = tag_ids.iter().find(|id| !allowed_tags.contains(id)) {
            tx.rollback().await?;
            return Err(ApiError::bad_request(
                "unknown_tag",
                format!("标签 {unknown} 不存在或已被归档"),
            ));
        }
    }

    let now = now_unix();
    let entry_id = if let Some(current) = &existing {
        sqlx::query("UPDATE entries SET content_md = ?, updated_at = ? WHERE id = ?")
            .bind(&request.content_md)
            .bind(now)
            .bind(current.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM entry_tags WHERE entry_id = ?")
            .bind(current.id)
            .execute(&mut *tx)
            .await?;
        current.id
    } else {
        let result = sqlx::query(
            "INSERT INTO entries (user_id, entry_date, content_md, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(user.id)
        .bind(&date)
        .bind(&request.content_md)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        result.last_insert_rowid()
    };

    for tag_id in tag_ids {
        sqlx::query("INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?, ?)")
            .bind(entry_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    let entry = fetch_entry(&state.pool, user.id, &date)
        .await?
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("刚写入的日记不存在")))?;
    Ok(Json(entry).into_response())
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(date): Path<String>,
) -> ApiResult<StatusCode> {
    validate_path_date(&date)?;
    sqlx::query("DELETE FROM entries WHERE user_id = ? AND entry_date = ?")
        .bind(user.id)
        .bind(&date)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn fetch_entry(
    pool: &SqlitePool,
    user_id: i64,
    date: &str,
) -> ApiResult<Option<EntryResponse>> {
    let Some(row) = sqlx::query_as::<_, EntryRow>(
        "SELECT id, entry_date, content_md, created_at, updated_at FROM entries \
         WHERE user_id = ? AND entry_date = ?",
    )
    .bind(user_id)
    .bind(date)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };

    let tags = sqlx::query_as::<_, TagBriefRow>(
        "SELECT et.entry_id, t.id, t.name, t.color, t.emoji \
         FROM entry_tags et JOIN tags t ON t.id = et.tag_id \
         WHERE et.entry_id = ? ORDER BY t.sort_order, t.name",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|tag| TagBrief {
        id: tag.id,
        name: tag.name,
        color: tag.color,
        emoji: tag.emoji,
    })
    .collect();

    Ok(Some(EntryResponse {
        date: row.entry_date,
        content_md: row.content_md,
        tags,
        created_at: to_rfc3339(row.created_at),
        updated_at: to_rfc3339(row.updated_at),
    }))
}

async fn load_tags_for_entries(
    pool: &SqlitePool,
    user_id: i64,
    entries: &[EntryRow],
) -> ApiResult<Vec<TagBriefRow>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = entries.iter().map(|entry| entry.id).collect();
    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT et.entry_id, t.id, t.name, t.color, t.emoji \
         FROM entry_tags et JOIN tags t ON t.id = et.tag_id \
         WHERE t.user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(" AND et.entry_id IN (");
    let mut separated = builder.separated(", ");
    for id in ids {
        separated.push_bind(id);
    }
    builder.push(") ORDER BY t.sort_order, t.name");
    Ok(builder
        .build_query_as::<TagBriefRow>()
        .fetch_all(pool)
        .await?)
}

fn validate_path_date(date: &str) -> ApiResult<()> {
    if valid_entry_date(date) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_date",
            "日期必须是 YYYY-MM-DD，且在允许范围内",
        ))
    }
}

fn validate_query_date(date: &str) -> ApiResult<String> {
    if valid_filter_date(date) {
        Ok(date.to_owned())
    } else {
        Err(ApiError::bad_request(
            "invalid_date",
            "日期必须是 YYYY-MM-DD",
        ))
    }
}
