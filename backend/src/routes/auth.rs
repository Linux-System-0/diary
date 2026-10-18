use std::sync::OnceLock;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use rand::{RngCore, rngs::OsRng};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use sqlx::FromRow;

use crate::{
    auth::{
        AuthUser, clear_refresh_cookie, hash_password, issue_access, issue_mfa, issue_refresh,
        password_needs_rehash, read_refresh_cookie, refresh_cookie, revoke_refresh, rotate_refresh,
        validate_password, verify_password,
    },
    error::{ApiError, ApiJson, ApiResult},
    rate_limit,
    state::AppState,
    util::{now_unix, to_rfc3339},
};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(FromRow)]
struct LoginUser {
    id: i64,
    password_hash: String,
    totp_enabled: bool,
    twofa_disable_at: Option<i64>,
    token_version: i64,
}

pub async fn register(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<RegisterRequest>,
) -> ApiResult<Response> {
    if !state.config.allow_registration {
        return Err(ApiError::forbidden("registration_disabled", "注册已关闭"));
    }

    if let Some(retry_after) = state
        .rate_limits
        .check_and_hit("register:global", 50, 3600, now_unix())
        .await
    {
        return Err(ApiError::too_many(retry_after));
    }

    let username = validate_username(&request.username)?;
    validate_password(&request.password)
        .map_err(|message| ApiError::bad_request("weak_password", message))?;

    let password_hash = hash_password(&request.password).map_err(ApiError::internal)?;
    let now = now_unix();
    let username_norm = username.to_lowercase();

    let result = sqlx::query(
        "INSERT INTO users (username, username_norm, password_hash, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&username)
    .bind(&username_norm)
    .bind(&password_hash)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => Ok((StatusCode::CREATED, Json(json!({ "ok": true }))).into_response()),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            Err(ApiError::conflict("username_taken", "用户名已被使用"))
        }
        Err(error) => Err(ApiError::from(error)),
    }
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<LoginRequest>,
) -> ApiResult<Response> {
    let username_norm = request.username.trim().to_lowercase();
    let now = now_unix();
    let login_key = rate_limit::login_key(&username_norm);
    if let Some(retry_after) = state.rate_limits.locked_for(&login_key, now).await {
        return Err(ApiError::too_many(retry_after));
    }

    let user = sqlx::query_as::<_, LoginUser>(
        "SELECT id, password_hash, totp_enabled, twofa_disable_at, token_version \
         FROM users WHERE username_norm = ?",
    )
    .bind(&username_norm)
    .fetch_optional(&state.pool)
    .await?;

    let Some(mut user) = user else {
        // Equalize timing so usernames cannot be enumerated.
        verify_password(&request.password, dummy_hash());
        state
            .rate_limits
            .record_failure(&login_key, 20, 3600, 900, now)
            .await;
        return Err(invalid_credentials());
    };

    if !verify_password(&request.password, &user.password_hash) {
        state
            .rate_limits
            .record_failure(&login_key, 20, 3600, 900, now)
            .await;
        return Err(invalid_credentials());
    }
    state.rate_limits.clear(&login_key).await;

    if password_needs_rehash(&user.password_hash) {
        let upgraded = hash_password(&request.password).map_err(ApiError::internal)?;
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(&upgraded)
            .bind(now_unix())
            .bind(user.id)
            .execute(&state.pool)
            .await?;
    }

    if user.totp_enabled
        && user
            .twofa_disable_at
            .is_some_and(|deadline| now_unix() >= deadline)
    {
        sqlx::query(
            "UPDATE users SET totp_enabled = 0, totp_secret_enc = NULL, \
             totp_pending_at = NULL, twofa_disable_at = NULL, \
             token_version = token_version + 1, updated_at = ? WHERE id = ?",
        )
        .bind(now_unix())
        .bind(user.id)
        .execute(&state.pool)
        .await?;
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = ? \
             WHERE user_id = ? AND revoked_at IS NULL",
        )
        .bind(now_unix())
        .bind(user.id)
        .execute(&state.pool)
        .await?;
        tracing::info!(user_id = user.id, "2FA 冷静期到期，已自动关闭 2FA");
        user.totp_enabled = false;
        user.token_version += 1;
    }

    if user.totp_enabled {
        let mfa = issue_mfa(
            state.config.jwt_secret.expose_secret(),
            user.id,
            state.config.mfa_ttl,
        )?;
        return Ok(no_store(
            Json(json!({
                "mfa_required": true,
                "mfa_token": mfa.token,
                "expires_in": mfa.expires_in,
            }))
            .into_response(),
        ));
    }

    let (access, refresh) = issue_session(&state, user.id, user.token_version).await?;
    let mut response = Json(json!({
        "access_token": access.token,
        "expires_in": access.expires_in,
    }))
    .into_response();
    set_cookie(&mut response, refresh);
    Ok(no_store(response))
}

pub async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let Some(raw) = read_refresh_cookie(&headers) else {
        return Err(ApiError::unauthorized(
            "invalid_token",
            "登录状态无效或已过期",
        ));
    };

    match rotate_refresh(&state.pool, &raw, state.config.refresh_ttl).await {
        Ok(rotation) => {
            let access = issue_access(
                state.config.jwt_secret.expose_secret(),
                rotation.user_id,
                rotation.token_version,
                state.config.access_ttl,
            )?;
            let max_age = (rotation.expires_at - now_unix()).max(1) as u64;
            let mut response = Json(json!({
                "access_token": access.token,
                "expires_in": access.expires_in,
            }))
            .into_response();
            set_cookie(
                &mut response,
                refresh_cookie(&state.config, &rotation.raw_token, max_age),
            );
            Ok(no_store(response))
        }
        Err(error) => {
            let mut response = error.into_response();
            response
                .headers_mut()
                .insert(header::SET_COOKIE, clear_refresh_cookie(&state.config));
            Ok(response)
        }
    }
}

pub async fn logout(
    State(state): State<AppState>,
    _user: AuthUser,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if let Some(raw) = read_refresh_cookie(&headers) {
        revoke_refresh(&state.pool, &raw).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear_refresh_cookie(&state.config));
    Ok(no_store(response))
}

pub async fn me(_user: AuthUser) -> Response {
    no_store(
        Json(json!({
            "id": _user.id,
            "username": _user.username,
            "totp_enabled": _user.totp_enabled,
            "twofa_disable_at": _user.twofa_disable_at.map(to_rfc3339),
        }))
        .into_response(),
    )
}

pub async fn issue_session(
    state: &AppState,
    user_id: i64,
    token_version: i64,
) -> ApiResult<(crate::auth::IssuedToken, HeaderValue)> {
    let access = issue_access(
        state.config.jwt_secret.expose_secret(),
        user_id,
        token_version,
        state.config.access_ttl,
    )?;
    let (raw, _expires_at) = issue_refresh(&state.pool, user_id, state.config.refresh_ttl).await?;
    let cookie = refresh_cookie(&state.config, &raw, state.config.refresh_ttl.as_secs());
    Ok((access, cookie))
}

pub(crate) fn set_cookie(response: &mut Response, cookie: HeaderValue) {
    response.headers_mut().insert(header::SET_COOKIE, cookie);
}

pub(crate) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn invalid_credentials() -> ApiError {
    ApiError::unauthorized("invalid_credentials", "用户名或密码错误")
}

fn validate_username(username: &str) -> ApiResult<String> {
    let username = username.trim().to_owned();
    let valid = (3..=32).contains(&username.len())
        && username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_username",
            "用户名需为 3–32 位字母、数字、下划线、点或短横线",
        ));
    }
    Ok(username)
}

fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        hash_password(&hex::encode(bytes)).expect("dummy hash")
    })
}
