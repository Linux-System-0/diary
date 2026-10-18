use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroizing;

use crate::{
    auth::{
        AuthUser, UserRow, apply_twofa_disable_deadline, build_totp, check_totp_now,
        decrypt_secret, encrypt_secret, generate_totp_secret, hash_password, load_user,
        normalize_totp_code, password_needs_rehash, totp_secret_base32, totp_uri,
        validate_password, verify_password,
    },
    error::{ApiError, ApiJson, ApiResult},
    rate_limit,
    routes::auth::{issue_session, no_store, set_cookie},
    state::AppState,
    util::{now_unix, to_rfc3339},
};

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub mfa_token: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct CodeRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct DisableRequest {
    pub password: String,
}

#[derive(Deserialize)]
pub struct PasswordRequest {
    pub current_password: String,
    pub new_password: String,
    pub totp_code: Option<String>,
}

pub async fn setup(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let row = load_user_or_401(&state, user.id).await?;
    if row.totp_enabled {
        return Err(ApiError::conflict("already_enabled", "两步验证已开启"));
    }

    // Reuse an unconfirmed pending secret so reloading the page does not
    // invalidate an already-scanned QR code.
    let secret = match (&row.totp_secret_enc, row.totp_pending_at) {
        (Some(encrypted), Some(_)) => Zeroizing::new(
            decrypt_secret(state.config.master_key.expose_secret(), encrypted)
                .map_err(ApiError::internal)?,
        ),
        _ => Zeroizing::new(generate_totp_secret().to_vec()),
    };

    let totp =
        build_totp(&secret, &user.username, state.config.totp_skew).map_err(ApiError::internal)?;
    let uri = totp_uri(&totp).map_err(ApiError::internal)?;
    let base32 = totp_secret_base32(&totp);

    let encrypted = encrypt_secret(state.config.master_key.expose_secret(), &secret)
        .map_err(ApiError::internal)?;
    sqlx::query(
        "UPDATE users SET totp_secret_enc = ?, totp_pending_at = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&encrypted)
    .bind(now_unix())
    .bind(now_unix())
    .bind(user.id)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({
        "otpauth_uri": uri,
        "secret_base32": base32,
    })))
}

pub async fn enable(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<CodeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = load_user_or_401(&state, user.id).await?;
    if row.totp_enabled {
        return Err(ApiError::conflict("already_enabled", "两步验证已开启"));
    }
    let Some(encrypted) = row.totp_secret_enc.as_deref() else {
        return Err(ApiError::bad_request(
            "setup_required",
            "请先获取两步验证密钥",
        ));
    };

    let secret = Zeroizing::new(
        decrypt_secret(state.config.master_key.expose_secret(), encrypted)
            .map_err(ApiError::internal)?,
    );
    let totp =
        build_totp(&secret, &user.username, state.config.totp_skew).map_err(ApiError::internal)?;
    let step = check_totp_now(&totp, &request.code).ok_or_else(invalid_totp)?;

    let now = now_unix();
    let changed = sqlx::query(
        "UPDATE users SET totp_enabled = 1, totp_last_step = ?, totp_pending_at = NULL, \
         updated_at = ? WHERE id = ? AND totp_enabled = 0 AND totp_last_step < ?",
    )
    .bind(step as i64)
    .bind(now)
    .bind(user.id)
    .bind(step as i64)
    .execute(&state.pool)
    .await?
    .rows_affected();

    if changed == 0 {
        return Err(code_already_used());
    }

    Ok(Json(json!({ "ok": true })))
}

pub async fn verify(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<VerifyRequest>,
) -> ApiResult<Response> {
    let user_id =
        crate::auth::decode_mfa(state.config.jwt_secret.expose_secret(), &request.mfa_token)?;
    let row = load_user_or_401(&state, user_id).await?;
    let now = now_unix();
    let limit_key = rate_limit::twofa_key(row.id);
    if let Some(retry_after) = state.rate_limits.locked_for(&limit_key, now).await {
        return Err(ApiError::too_many(retry_after));
    }

    if apply_twofa_disable_deadline(&state.pool, &row, now_unix()).await? {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "两步验证已关闭，请重新登录",
        ));
    }
    if !row.totp_enabled {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    }

    let step = match verify_user_code(&state, &row, &request.code) {
        Ok(step) => {
            state.rate_limits.clear(&limit_key).await;
            step
        }
        Err(error) => {
            if error.code() == "invalid_totp" {
                state
                    .rate_limits
                    .record_failure(&limit_key, 10, 3600, 0, now)
                    .await;
            }
            return Err(error);
        }
    };
    let changed = sqlx::query(
        "UPDATE users SET totp_last_step = ?, updated_at = ? WHERE id = ? AND totp_last_step < ?",
    )
    .bind(step as i64)
    .bind(now_unix())
    .bind(row.id)
    .bind(step as i64)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(code_already_used());
    }

    let (access, cookie) = issue_session(&state, row.id, row.token_version).await?;
    let mut response = Json(json!({
        "access_token": access.token,
        "expires_in": access.expires_in,
    }))
    .into_response();
    set_cookie(&mut response, cookie);
    Ok(no_store(response))
}

pub async fn disable(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<DisableRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let row = load_user_or_401(&state, user.id).await?;
    let limit_key = rate_limit::disable_key(user.id);
    if let Some(retry_after) = state
        .rate_limits
        .check_and_hit(&limit_key, 3, 24 * 60 * 60, now_unix())
        .await
    {
        return Err(ApiError::too_many(retry_after));
    }
    if !verify_password(&request.password, &row.password_hash) {
        return Err(ApiError::unauthorized("invalid_credentials", "密码错误"));
    }
    if !row.totp_enabled {
        return Err(ApiError::conflict("not_enabled", "两步验证未开启"));
    }

    let effective_at = now_unix()
        + i64::try_from(state.config.twofa_disable_delay.as_secs()).unwrap_or(i64::MAX / 2);
    sqlx::query("UPDATE users SET twofa_disable_at = ?, updated_at = ? WHERE id = ?")
        .bind(effective_at)
        .bind(now_unix())
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({
        "effective_at": to_rfc3339(effective_at),
    })))
}

pub async fn cancel_disable(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<StatusCode> {
    sqlx::query("UPDATE users SET twofa_disable_at = NULL, updated_at = ? WHERE id = ?")
        .bind(now_unix())
        .bind(user.id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    ApiJson(request): ApiJson<PasswordRequest>,
) -> ApiResult<Response> {
    let row = load_user_or_401(&state, user.id).await?;
    if !verify_password(&request.current_password, &row.password_hash) {
        return Err(ApiError::unauthorized(
            "invalid_credentials",
            "当前密码错误",
        ));
    }
    validate_password(&request.new_password)
        .map_err(|message| ApiError::bad_request("weak_password", message))?;

    let mut consumed_step: Option<u64> = None;
    if row.totp_enabled {
        let Some(code) = request.totp_code.as_deref() else {
            return Err(ApiError::unauthorized("invalid_totp", "请输入两步验证码"));
        };
        let step = verify_user_code(&state, &row, code)?;
        consumed_step = Some(step);
    }

    let new_hash = hash_password(&request.new_password).map_err(ApiError::internal)?;
    let now = now_unix();
    let mut tx = state.pool.begin().await?;
    let changed = if let Some(step) = consumed_step {
        sqlx::query(
            "UPDATE users SET password_hash = ?, token_version = token_version + 1, \
             totp_last_step = ?, updated_at = ? WHERE id = ? AND totp_last_step < ?",
        )
        .bind(&new_hash)
        .bind(step as i64)
        .bind(now)
        .bind(user.id)
        .bind(step as i64)
        .execute(&mut *tx)
        .await?
        .rows_affected()
    } else {
        sqlx::query(
            "UPDATE users SET password_hash = ?, token_version = token_version + 1, \
             updated_at = ? WHERE id = ?",
        )
        .bind(&new_hash)
        .bind(now)
        .bind(user.id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
    };
    if changed == 0 {
        tx.rollback().await?;
        return Err(code_already_used());
    }
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        crate::auth::clear_refresh_cookie(&state.config),
    );
    Ok(no_store(response))
}

/// Used by the future "change password" UI: re-hash after verifying the old one
/// so stale Argon2 parameters never linger.
pub async fn rehash_if_needed(state: &AppState, user_id: i64, password: &str) -> ApiResult<()> {
    let row = load_user_or_401(state, user_id).await?;
    if password_needs_rehash(&row.password_hash) {
        let hash = hash_password(password).map_err(ApiError::internal)?;
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(hash)
            .bind(now_unix())
            .bind(user_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(())
}

async fn load_user_or_401(state: &AppState, user_id: i64) -> ApiResult<UserRow> {
    load_user(&state.pool, user_id)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid_token", "登录状态无效或已过期"))
}

fn verify_user_code(state: &AppState, row: &UserRow, code: &str) -> ApiResult<u64> {
    let Some(encrypted) = row.totp_secret_enc.as_deref() else {
        return Err(ApiError::bad_request("setup_required", "两步验证尚未配置"));
    };
    let secret = Zeroizing::new(
        decrypt_secret(state.config.master_key.expose_secret(), encrypted)
            .map_err(ApiError::internal)?,
    );
    let totp =
        build_totp(&secret, &row.username, state.config.totp_skew).map_err(ApiError::internal)?;
    if normalize_totp_code(code).is_none() {
        return Err(invalid_totp());
    }
    check_totp_now(&totp, code).ok_or_else(invalid_totp)
}

fn invalid_totp() -> ApiError {
    ApiError::unauthorized("invalid_totp", "两步验证码错误")
}

fn code_already_used() -> ApiError {
    ApiError::conflict("code_already_used", "该验证码已使用，请等待下一个验证码")
}
