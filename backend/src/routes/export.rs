use std::collections::{HashMap, HashSet};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use serde::Deserialize;
use sqlx::FromRow;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiJson, ApiResult},
    format::{DiaryEntry, DiaryPayload, DiaryTag},
    state::AppState,
    util::{now_unix, to_rfc3339, valid_filter_date},
};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    All,
    Dates,
}

#[derive(Deserialize)]
pub struct ExportRequest {
    pub scope: Scope,
    #[serde(default)]
    pub dates: Vec<String>,
}

#[derive(FromRow)]
struct EntryRow {
    id: i64,
    entry_date: String,
    content_md: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(FromRow)]
struct TagRow {
    entry_id: i64,
    id: i64,
    name: String,
    color: Option<String>,
    emoji: Option<String>,
}

pub async fn prepare(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<ExportRequest>,
) -> ApiResult<Response> {
    let dates = normalize_dates(request.scope, request.dates)?;
    let entries = load_entries(&state, user.id, request.scope, &dates).await?;
    let tag_rows = load_entry_tags(&state, user.id, &entries).await?;

    let mut tag_lookup: HashMap<i64, DiaryTag> = HashMap::new();
    for row in &tag_rows {
        tag_lookup.entry(row.id).or_insert_with(|| DiaryTag {
            name: row.name.clone(),
            color: row.color.clone(),
            emoji: row.emoji.clone(),
        });
    }

    let mut tags = tag_lookup.into_values().collect::<Vec<_>>();
    tags.sort_by(|a, b| a.name.cmp(&b.name));

    let mut tags_by_entry: HashMap<i64, Vec<String>> = HashMap::new();
    for row in tag_rows {
        tags_by_entry.entry(row.entry_id).or_default().push(row.name);
    }

    let payload = DiaryPayload {
        format: "diary".into(),
        version: 1,
        exported_at: to_rfc3339(now_unix()),
        app: format!("diary/{}", env!("CARGO_PKG_VERSION")),
        timezone: "UTC".into(),
        tags,
        entries: entries
            .into_iter()
            .map(|entry| DiaryEntry {
                date: entry.entry_date.clone(),
                content_md: entry.content_md,
                tags: tags_by_entry.remove(&entry.id).unwrap_or_default(),
                created_at: Some(to_rfc3339(entry.created_at)),
                updated_at: Some(to_rfc3339(entry.updated_at)),
            })
            .collect(),
    };

    let body = serde_json::to_vec(&payload).map_err(ApiError::internal)?;
    if body.len() > state.config.max_export_bytes {
        return Err(ApiError::too_large(
            "export_too_large",
            "导出内容超过上限，请缩小日期范围",
        ));
    }

    let len = body.len();
    let mut response = Response::new(Body::from(body));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(&len.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}

fn normalize_dates(scope: Scope, dates: Vec<String>) -> ApiResult<Vec<String>> {
    if matches!(scope, Scope::All) {
        return Ok(Vec::new());
    }
    if dates.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_dates",
            "scope=dates 时 dates 不能为空",
        ));
    }
    if dates.len() > 100_000 {
        return Err(ApiError::bad_request(
            "invalid_dates",
            "日期数量超过上限",
        ));
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(dates.len());
    for date in dates {
        if !valid_filter_date(&date) {
            return Err(ApiError::bad_request(
                "invalid_date",
                format!("非法日期: {date}"),
            ));
        }
        if seen.insert(date.clone()) {
            normalized.push(date);
        }
    }
    Ok(normalized)
}

async fn load_entries(
    state: &AppState,
    user_id: i64,
    scope: Scope,
    dates: &[String],
) -> ApiResult<Vec<EntryRow>> {
    if matches!(scope, Scope::All) {
        return Ok(sqlx::query_as::<_, EntryRow>(
            "SELECT id, entry_date, content_md, created_at, updated_at FROM entries \
             WHERE user_id = ? ORDER BY entry_date",
        )
        .bind(user_id)
        .fetch_all(&state.pool)
        .await?);
    }

    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, entry_date, content_md, created_at, updated_at FROM entries \
         WHERE user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(" AND entry_date IN (");
    let mut separated = builder.separated(", ");
    for date in dates {
        separated.push_bind(date);
    }
    builder.push(") ORDER BY entry_date");
    Ok(builder.build_query_as::<EntryRow>().fetch_all(&state.pool).await?)
}

async fn load_entry_tags(
    state: &AppState,
    user_id: i64,
    entries: &[EntryRow],
) -> ApiResult<Vec<TagRow>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT et.entry_id, t.id, t.name, t.color, t.emoji \
         FROM entry_tags et JOIN tags t ON t.id = et.tag_id \
         WHERE t.user_id = ",
    );
    builder.push_bind(user_id);
    builder.push(" AND et.entry_id IN (");
    let mut separated = builder.separated(", ");
    for entry in entries {
        separated.push_bind(entry.id);
    }
    builder.push(") ORDER BY t.sort_order, t.name");
    Ok(builder.build_query_as::<TagRow>().fetch_all(&state.pool).await?)
}
