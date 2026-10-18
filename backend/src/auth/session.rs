use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    auth::UserRow,
    error::{ApiError, ApiResult},
    util::now_unix,
};

#[derive(Debug, FromRow)]
struct RefreshRow {
    id: String,
    user_id: i64,
    expires_at: i64,
    revoked_at: Option<i64>,
    replaced_by: Option<String>,
}

pub struct Rotation {
    pub user_id: i64,
    pub token_version: i64,
    pub raw_token: String,
    pub expires_at: i64,
}

fn new_raw_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn token_hash(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

pub async fn issue_refresh(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    ttl: Duration,
) -> ApiResult<(String, i64)> {
    let raw = new_raw_token();
    let id = Uuid::new_v4().to_string();
    let now = now_unix();
    let expires_at = now + ttl.as_secs() as i64;

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(token_hash(&raw))
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;

    Ok((raw, expires_at))
}

pub async fn revoke_refresh(pool: &sqlx::SqlitePool, raw: &str) -> ApiResult<()> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ? WHERE token_hash = ? AND revoked_at IS NULL",
    )
    .bind(now_unix())
    .bind(token_hash(raw))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn revoke_all_for_user(pool: &sqlx::SqlitePool, user_id: i64) -> ApiResult<()> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
    )
    .bind(now_unix())
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically rotates a refresh token. Reuse of an already-rotated token is
/// treated as theft: all sessions are revoked and `token_version` is bumped.
pub async fn rotate_refresh(
    pool: &sqlx::SqlitePool,
    raw: &str,
    ttl: Duration,
) -> ApiResult<Rotation> {
    let now = now_unix();
    let mut tx = pool.begin().await?;

    let row = sqlx::query_as::<_, RefreshRow>(
        "SELECT id, user_id, expires_at, revoked_at, replaced_by FROM refresh_tokens \
         WHERE token_hash = ?",
    )
    .bind(token_hash(raw))
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        tx.rollback().await?;
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    };

    if row.replaced_by.is_some() {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(row.user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE users SET token_version = token_version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(row.user_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::warn!(user_id = row.user_id, "检测到刷新令牌复用，已吊销全部会话");
        return Err(ApiError::unauthorized(
            "token_reused",
            "检测到可疑登录，请重新登录",
        ));
    }

    if row.revoked_at.is_some() || row.expires_at <= now {
        tx.rollback().await?;
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }

    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, password_hash, totp_secret_enc, totp_enabled, \
         totp_last_step, totp_pending_at, twofa_disable_at, token_version \
         FROM users WHERE id = ?",
    )
    .bind(row.user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(user) = user else {
        tx.rollback().await?;
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    };

    if user.totp_enabled
        && user
            .twofa_disable_at
            .is_some_and(|deadline| now >= deadline)
    {
        sqlx::query(
            "UPDATE users SET totp_enabled = 0, totp_secret_enc = NULL, \
             totp_pending_at = NULL, twofa_disable_at = NULL, \
             token_version = token_version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(user_id = user.id, "2FA 冷静期到期，已自动关闭 2FA");
        return Err(ApiError::unauthorized(
            "invalid_token",
            "两步验证已关闭，请重新登录",
        ));
    }

    let raw_new = new_raw_token();
    let new_id = Uuid::new_v4().to_string();
    let expires_at = now + ttl.as_secs() as i64;

    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(row.user_id)
    .bind(token_hash(&raw_new))
    .bind(expires_at)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    let updated = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ?, replaced_by = ? \
         WHERE id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(&new_id)
    .bind(&row.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if updated != 1 {
        tx.rollback().await?;
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }

    tx.commit().await?;
    Ok(Rotation {
        user_id: row.user_id,
        token_version: user.token_version,
        raw_token: raw_new,
        expires_at,
    })
}
