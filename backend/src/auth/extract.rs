use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use secrecy::ExposeSecret;
use sqlx::FromRow;

use crate::{
    auth::{Decoded, decode_access},
    error::ApiError,
    state::AppState,
    util::now_unix,
};

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub totp_secret_enc: Option<Vec<u8>>,
    pub totp_enabled: bool,
    pub totp_last_step: i64,
    pub totp_pending_at: Option<i64>,
    pub twofa_disable_at: Option<i64>,
    pub token_version: i64,
}

pub async fn load_user(
    pool: &sqlx::SqlitePool,
    user_id: i64,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, totp_secret_enc, totp_enabled, \
         totp_last_step, totp_pending_at, twofa_disable_at, token_version \
         FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Applies the lazy 2FA-disable deadline. Returns true when 2FA was disabled by
/// this call (the caller must then force a fresh login).
pub async fn apply_twofa_disable_deadline(
    pool: &sqlx::SqlitePool,
    user: &UserRow,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let Some(deadline) = user.twofa_disable_at else {
        return Ok(false);
    };
    if now < deadline || !user.totp_enabled {
        return Ok(false);
    }

    let changed = sqlx::query(
        "UPDATE users SET totp_enabled = 0, totp_secret_enc = NULL, totp_pending_at = NULL, \
         twofa_disable_at = NULL, token_version = token_version + 1, updated_at = ? \
         WHERE id = ? AND twofa_disable_at IS NOT NULL AND twofa_disable_at <= ?",
    )
    .bind(now)
    .bind(user.id)
    .bind(now)
    .execute(pool)
    .await?
    .rows_affected();

    if changed > 0 {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ? \
             WHERE user_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(user.id)
        .execute(pool)
        .await?;
        tracing::info!(user_id = user.id, "2FA 冷静期到期，已自动关闭 2FA");
    }
    Ok(changed > 0)
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub totp_enabled: bool,
    pub twofa_disable_at: Option<i64>,
    pub token_version: i64,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts)?;
        let decoded = decode_access(state.config.jwt_secret.expose_secret(), token)?;
        let Decoded::Access {
            user_id,
            token_version,
            ..
        } = decoded
        else {
            return Err(ApiError::unauthorized("mfa_required", "请先完成两步验证"));
        };

        let user = load_user(&state.pool, user_id)
            .await?
            .ok_or_else(|| ApiError::unauthorized("invalid_token", "登录状态无效或已过期"))?;

        if user.token_version != token_version {
            return Err(ApiError::unauthorized(
                "invalid_token",
                "登录状态无效或已过期",
            ));
        }

        if apply_twofa_disable_deadline(&state.pool, &user, now_unix()).await? {
            return Err(ApiError::unauthorized(
                "invalid_token",
                "两步验证已关闭，请重新登录",
            ));
        }

        Ok(Self {
            id: user.id,
            username: user.username,
            totp_enabled: user.totp_enabled,
            twofa_disable_at: user.twofa_disable_at,
            token_version: user.token_version,
        })
    }
}

pub fn bearer_token(parts: &Parts) -> Result<&str, ApiError> {
    let value = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("invalid_token", "缺少访问令牌"))?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .ok_or_else(|| ApiError::unauthorized("invalid_token", "访问令牌格式错误"))?;
    if token.is_empty() {
        return Err(ApiError::unauthorized("invalid_token", "访问令牌为空"));
    }
    Ok(token)
}
