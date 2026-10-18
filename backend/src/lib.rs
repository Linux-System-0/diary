pub mod auth;
pub mod config;
pub mod db;
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
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header},
    routing::{get, patch, post},
};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::Config, state::AppState};

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

    let auth = Router::new()
        .route("/auth/register", post(routes::auth::register))
        .route("/auth/login", post(routes::auth::login))
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
        .route("/auth/2fa/verify", post(routes::twofa::verify))
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

    let api = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .merge(auth)
        .merge(twofa)
        .merge(tags)
        .merge(entries)
        .merge(export)
        .merge(import);

    Ok(Router::new()
        .nest("/api/v1", api.clone())
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(60),
        ))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state))
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
        tokio::signal::ctrl_c()
            .await
            .expect("无法监听 Ctrl-C");
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
