use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::FromRow;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    auth::AuthUser,
    error::{ApiError, ApiJson, ApiResult},
    state::AppState,
    util::now_unix,
};

#[derive(Debug, FromRow)]
struct TagRow {
    id: i64,
    name: String,
    color: Option<String>,
    emoji: Option<String>,
    sort_order: i64,
    deleted_at: Option<i64>,
}

#[derive(Serialize)]
pub struct TagResponse {
    id: i64,
    name: String,
    color: Option<String>,
    emoji: Option<String>,
    sort_order: i64,
    archived: bool,
}

impl From<TagRow> for TagResponse {
    fn from(row: TagRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            color: row.color,
            emoji: row.emoji,
            sort_order: row.sort_order,
            archived: row.deleted_at.is_some(),
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Deserialize)]
pub struct CreateTagRequest {
    pub name: String,
    pub color: Option<String>,
    pub emoji: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateTagRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub color: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub emoji: Option<Option<String>>,
    pub sort_order: Option<i64>,
}

/// Distinguishes "field absent" (outer `None`) from "field explicitly null"
/// (`Some(None)`) in PATCH bodies.
fn deserialize_optional_field<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<TagResponse>>> {
    let rows = if query.include_archived {
        sqlx::query_as::<_, TagRow>(
            "SELECT id, name, color, emoji, sort_order, deleted_at FROM tags \
             WHERE user_id = ? ORDER BY sort_order, name",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, TagRow>(
            "SELECT id, name, color, emoji, sort_order, deleted_at FROM tags \
             WHERE user_id = ? AND deleted_at IS NULL ORDER BY sort_order, name",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(rows.into_iter().map(TagResponse::from).collect()))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<CreateTagRequest>,
) -> ApiResult<Response> {
    let name = validate_name(&request.name)?;
    let color = request.color.as_deref().map(validate_color).transpose()?;
    let emoji = request.emoji.as_deref().map(validate_emoji).transpose()?;
    let sort_order = request.sort_order.unwrap_or(0);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tags WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;
    if count >= state.config.max_tags {
        return Err(ApiError::conflict(
            "tag_limit",
            format!("最多只能创建 {} 个标签", state.config.max_tags),
        ));
    }

    let result = sqlx::query(
        "INSERT INTO tags (user_id, name, name_norm, color, emoji, sort_order, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(user.id)
    .bind(&name)
    .bind(name.to_lowercase())
    .bind(&color)
    .bind(&emoji)
    .bind(sort_order)
    .bind(now_unix())
    .execute(&state.pool)
    .await;

    let id = match result {
        Ok(result) => result.last_insert_rowid(),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            return Err(ApiError::conflict("tag_exists", "标签名已存在"));
        }
        Err(error) => return Err(ApiError::from(error)),
    };

    Ok((
        StatusCode::CREATED,
        Json(TagResponse {
            id,
            name,
            color,
            emoji,
            sort_order,
            archived: false,
        }),
    )
        .into_response())
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    ApiJson(request): ApiJson<UpdateTagRequest>,
) -> ApiResult<Json<TagResponse>> {
    let mut row = load_tag(&state, user.id, id)
        .await?
        .ok_or_else(|| ApiError::not_found("tag_not_found", "标签不存在"))?;

    if let Some(name) = request.name.as_deref() {
        row.name = validate_name(name)?;
    }
    if let Some(color) = request.color {
        row.color = color.as_deref().map(validate_color).transpose()?;
    }
    if let Some(emoji) = request.emoji {
        row.emoji = emoji.as_deref().map(validate_emoji).transpose()?;
    }
    if let Some(sort_order) = request.sort_order {
        row.sort_order = sort_order;
    }

    let result = sqlx::query(
        "UPDATE tags SET name = ?, name_norm = ?, color = ?, emoji = ?, sort_order = ? \
         WHERE id = ? AND user_id = ?",
    )
    .bind(&row.name)
    .bind(row.name.to_lowercase())
    .bind(&row.color)
    .bind(&row.emoji)
    .bind(row.sort_order)
    .bind(id)
    .bind(user.id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => Ok(Json(TagResponse::from(row))),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            Err(ApiError::conflict("tag_exists", "标签名已存在"))
        }
        Err(error) => Err(ApiError::from(error)),
    }
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let result = sqlx::query(
        "UPDATE tags SET deleted_at = ? WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
    )
    .bind(now_unix())
    .bind(id)
    .bind(user.id)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tags WHERE id = ? AND user_id = ?)")
                .bind(id)
                .bind(user.id)
                .fetch_one(&state.pool)
                .await?;
        if !exists {
            return Err(ApiError::not_found("tag_not_found", "标签不存在"));
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn load_tag(state: &AppState, user_id: i64, id: i64) -> ApiResult<Option<TagRow>> {
    Ok(sqlx::query_as::<_, TagRow>(
        "SELECT id, name, color, emoji, sort_order, deleted_at FROM tags \
         WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?)
}

fn validate_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let len = name.chars().count();
    if !(1..=32).contains(&len) {
        return Err(ApiError::bad_request(
            "invalid_tag_name",
            "标签名需为 1–32 个字符",
        ));
    }
    Ok(name.to_owned())
}

fn validate_color(color: &str) -> ApiResult<String> {
    let color = color.trim().to_ascii_lowercase();
    let valid = color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_color",
            "颜色格式必须为 #RRGGBB",
        ));
    }
    Ok(color)
}

fn validate_emoji(emoji: &str) -> ApiResult<String> {
    let emoji = emoji.trim();
    if emoji.is_empty()
        || emoji.len() > 8
        || UnicodeSegmentation::graphemes(emoji, true).count() != 1
    {
        return Err(ApiError::bad_request(
            "invalid_emoji",
            "emoji 必须是单个字符",
        ));
    }
    Ok(emoji.to_owned())
}
