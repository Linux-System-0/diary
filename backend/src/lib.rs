pub mod auth;
pub mod config;
pub mod db;
pub mod diary_file;
pub mod error;
pub mod format;
pub mod import_store;
pub mod rate_limit;
pub mod routes;
pub mod state;
pub mod util;

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use secrecy::ExposeSecret;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_governor::{
    GovernorError, GovernorLayer, governor::GovernorConfigBuilder,
    key_extractor::SmartIpKeyExtractor,
};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    auth::{Decoded, decode_access},
    config::Config,
    error::ApiError,
    state::AppState,
};

pub fn init_tracing(filter: &str) {
    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

pub fn build_state(config: Config, pool: SqlitePool) -> AppState {
    AppState {
        config: Arc::new(config),
        pool,
        imports: import_store::ImportStore::new(),
        rate_limits: std::sync::Arc::new(rate_limit::RateLimits::new()),
    }
}

pub fn app(state: AppState) -> Result<Router> {
    let origins = state
        .config
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse::<HeaderValue>()
                .with_context(|| format!("CORS origin 非法: {origin}"))
        })
        .collect::<Result<Vec<_>>>()?;

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]);

    let register_limit = GovernorLayer::new(
        GovernorConfigBuilder::default()
            .key_extractor(SmartIpKeyExtractor)
            .per_second(3600)
            .burst_size(5)
            .finish()
            .expect("注册限流配置合法"),
    )
    .error_handler(governor_error);
    let login_limit = GovernorLayer::new(
        GovernorConfigBuilder::default()
            .key_extractor(SmartIpKeyExtractor)
            .per_second(300)
            .burst_size(10)
            .finish()
            .expect("登录限流配置合法"),
    )
    .error_handler(governor_error);
    let verify_limit = GovernorLayer::new(
        GovernorConfigBuilder::default()
            .key_extractor(SmartIpKeyExtractor)
            .per_second(300)
            .burst_size(10)
            .finish()
            .expect("2FA 限流配置合法"),
    )
    .error_handler(governor_error);

    let auth = Router::new()
        .route(
            "/auth/register",
            post(routes::auth::register).route_layer(register_limit),
        )
        .route(
            "/auth/login",
            post(routes::auth::login).route_layer(login_limit),
        )
        .route("/auth/refresh", post(routes::auth::refresh))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/auth/me", get(routes::auth::me));

    let tags = Router::new()
        .route("/tags", get(routes::tags::list).post(routes::tags::create))
        .route(
            "/tags/{id}",
            patch(routes::tags::update).delete(routes::tags::delete),
        );

    let twofa = Router::new()
        .route(
            "/auth/2fa/verify",
            post(routes::twofa::verify).route_layer(verify_limit),
        )
        .route("/auth/2fa/setup", get(routes::twofa::setup))
        .route("/auth/2fa/enable", post(routes::twofa::enable))
        .route("/auth/2fa/disable", post(routes::twofa::disable))
        .route(
            "/auth/2fa/disable/cancel",
            post(routes::twofa::cancel_disable),
        )
        .route("/auth/password", post(routes::twofa::change_password));

    let export = Router::new().route("/export/prepare", post(routes::export::prepare));

    let import = Router::new()
        .route("/import/preview", post(routes::import::preview))
        .route("/import/commit", post(routes::import::commit))
        .route_layer(DefaultBodyLimit::max(state.config.max_import_bytes));

    let entries = Router::new()
        .route("/entries", get(routes::entries::list))
        .route(
            "/entries/{date}",
            get(routes::entries::get)
                .put(routes::entries::upsert)
                .delete(routes::entries::delete),
        );

    let business = Router::new()
        .merge(tags)
        .merge(entries)
        .merge(export)
        .merge(import)
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            write_rate_limit,
        ));

    let api = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .merge(auth)
        .merge(twofa)
        .merge(business)
        .fallback(api_not_found);

    Ok(Router::new()
        .nest("/api/v1", api.clone())
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(120),
        ))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(state.clone(), origin_guard))
        .with_state(state))
}

async fn api_not_found() -> ApiError {
    ApiError::not_found("not_found", "接口不存在")
}

fn governor_error(error: GovernorError) -> Response {
    let wait = match &error {
        GovernorError::TooManyRequests { wait_time, .. } => *wait_time,
        _ => 1,
    };
    ApiError::too_many(wait).into_response()
}

async fn origin_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let sensitive = request.uri().path().starts_with("/api/v1")
        && !matches!(request.uri().path(), "/api/v1/healthz" | "/api/v1/readyz");

    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .ok()
            .is_some_and(|origin| state.config.cors_origins.iter().any(|item| item == origin));
        if !allowed {
            return ApiError::forbidden("invalid_origin", "Origin 不在白名单").into_response();
        }
    }

    let mut response = next.run(request).await;
    if sensitive && !response.headers().contains_key(header::CACHE_CONTROL) {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

async fn write_rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let is_write = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if is_write
        && let Some(token) = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
        && let Ok(Decoded::Access { user_id, .. }) =
            decode_access(state.config.jwt_secret.expose_secret(), token)
        && let Some(retry_after) = state
            .rate_limits
            .check_and_hit(&rate_limit::write_key(user_id), 300, 60, rate_limit::now())
            .await
    {
        return ApiError::too_many(retry_after).into_response();
    }
    next.run(request).await
}

pub async fn run(config: Config) -> Result<()> {
    let pool = db::connect(&config).await?;
    let bind = config.bind;
    let app = app(build_state(config, pool))?;

    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("无法监听 {bind}"))?;
    tracing::info!(%bind, "diary 后端已启动");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("HTTP 服务异常退出")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("无法监听 Ctrl-C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("无法监听 SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("收到退出信号，正在关闭");
}
