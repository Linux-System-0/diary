use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use diary::{
    auth::build_totp,
    config::Config,
    db,
    state::AppState,
    util::now_unix,
};
use secrecy::SecretBox;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct TestApp {
    router: Router,
    state: AppState,
    _dir: Arc<TempDir>,
}

#[derive(Clone)]
struct Session {
    token: String,
    cookie: String,
}

impl TestApp {
    async fn new() -> Self {
        Self::with_config_overrides(|_| {}).await
    }

    async fn with_config_overrides(patch: impl FnOnce(&mut Config)) -> Self {
        let dir = Arc::new(TempDir::new().unwrap());
        let mut config = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            db_path: dir.path().join("test.db"),
            jwt_secret: SecretBox::new(Box::new(vec![7u8; 32])),
            master_key: SecretBox::new(Box::new([9u8; 32])),
            cors_origins: vec!["http://test.example".into()],
            allow_registration: true,
            secure_cookie: false,
            access_ttl: Duration::from_secs(900),
            refresh_ttl: Duration::from_secs(86_400),
            mfa_ttl: Duration::from_secs(300),
            totp_skew: 1,
            twofa_disable_delay: Duration::from_secs(7 * 24 * 3600),
            max_entry_bytes: 1_048_576,
            max_export_bytes: 67_108_864,
            max_import_bytes: 67_108_864,
            max_tags: 200,
            max_tags_per_entry: 20,
            rust_log: "info".into(),
        };
        patch(&mut config);
        let pool = db::connect(&config).await.unwrap();
        let state = diary::build_state(config, pool);
        let router = diary::app(state.clone()).unwrap();
        Self {
            router,
            state,
            _dir: dir,
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
        cookie: Option<&str>,
        origin: Option<&str>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 44444))));
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        let request = builder.body(Body::from(body)).unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8_lossy(&bytes).into_owned())
            })
        };
        (status, headers, value)
    }

    async fn register(&self, username: &str, password: &str) -> (StatusCode, Value) {
        let (status, _, value) = self
            .call(
                Method::POST,
                "/api/v1/auth/register",
                Some(json!({ "username": username, "password": password })),
                None,
                None,
                None,
            )
            .await;
        (status, value)
    }

    async fn login(&self, username: &str, password: &str) -> (StatusCode, HeaderMap, Value) {
        self.call(
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({ "username": username, "password": password })),
            None,
            None,
            None,
        )
        .await
    }

    async fn session(&self, username: &str, password: &str) -> Session {
        let (status, headers, value) = self.login(username, password).await;
        assert_eq!(status, StatusCode::OK, "login failed: {value}");
        Session {
            token: value["access_token"].as_str().unwrap().to_owned(),
            cookie: refresh_cookie(&headers),
        }
    }
}

fn refresh_cookie(headers: &HeaderMap) -> String {
    let value = headers
        .get(header::SET_COOKIE)
        .expect("Set-Cookie")
        .to_str()
        .unwrap();
    value.split(';').next().unwrap().to_owned()
}

fn code_at_step(secret_base32: &str, username: &str, step_offset: i64) -> String {
    let secret = totp_rs::Secret::try_from_base32(secret_base32).unwrap();
    let totp = build_totp(secret.as_bytes(), username, 1).unwrap();
    let step = now_unix() / 30 + step_offset;
    totp.generate((step as u64) * 30).to_string()
}

#[tokio::test]
async fn auth_register_login_refresh_rotation_and_reuse_detection() {
    let app = TestApp::new().await;
    let password = "correct horse battery staple";

    let (status, value) = app.register("alice", password).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    let (status, value) = app.register("ALICE", password).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "username_taken");

    let (status, _, value) = app.login("alice", "wrong password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "invalid_credentials");

    let session = app.session("alice", password).await;
    let (status, _, me) = app
        .call(Method::GET, "/api/v1/auth/me", None, Some(&session.token), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["username"], "alice");

    let (status, headers, refreshed) = app
        .call(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            Some(&session.cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    let new_token = refreshed["access_token"].as_str().unwrap().to_owned();
    let new_cookie = refresh_cookie(&headers);

    // Reusing the old, already-rotated cookie revokes every session and bumps
    // token_version, so even the freshly issued access token stops working.
    let (status, _, value) = app
        .call(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            Some(&session.cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "token_reused");

    let (status, _, value) = app
        .call(Method::GET, "/api/v1/auth/me", None, Some(&new_token), None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "invalid_token");

    let (status, _, _) = app
        .call(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            None,
            Some(&new_cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tags_and_entries_are_user_scoped_and_tags_must_be_active() {
    let app = TestApp::new().await;
    let password = "correct horse battery staple";
    app.register("alice", password).await;
    app.register("bob", password).await;
    let alice = app.session("alice", password).await;
    let bob = app.session("bob", password).await;

    let (status, _, tag) = app
        .call(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "name": "开心", "color": "#F59E0B", "emoji": "😄" })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{tag}");
    let tag_id = tag["id"].as_i64().unwrap();
    assert_eq!(tag["color"], "#f59e0b");

    let (status, _, duplicate) = app
        .call(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "name": " 开心 " })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(duplicate["error"]["code"], "tag_exists");

    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-02",
            Some(json!({ "content_md": "hello", "tag_ids": [tag_id] })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let updated_at = value["updated_at"].as_str().unwrap().to_owned();

    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-02",
            Some(json!({
                "content_md": "stale",
                "tag_ids": [],
                "base_updated_at": "2000-01-01T00:00:00Z"
            })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "stale_write");
    assert_eq!(value["error"]["details"]["date"], "2025-01-02");

    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-02",
            Some(json!({
                "content_md": "updated",
                "tag_ids": [],
                "base_updated_at": updated_at
            })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["content_md"], "updated");

    // Keep a second entry tagged before archiving it; editing that entry later
    // must preserve the now-archived tag instead of rejecting the save.
    let (status, _, _) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-05",
            Some(json!({ "content_md": "tagged", "tag_ids": [tag_id] })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Bob cannot reference Alice's tag.
    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-03",
            Some(json!({ "content_md": "bob", "tag_ids": [tag_id] })),
            Some(&bob.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "unknown_tag");

    let (status, _, _) = app
        .call(
            Method::DELETE,
            &format!("/api/v1/tags/{tag_id}"),
            None,
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-05",
            Some(json!({ "content_md": "still tagged", "tag_ids": [tag_id] })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["tags"][0]["id"], tag_id);

    let (status, _, value) = app
        .call(
            Method::PUT,
            "/api/v1/entries/2025-01-04",
            Some(json!({ "content_md": "archived", "tag_ids": [tag_id] })),
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "unknown_tag");

    let (status, _, list) = app
        .call(
            Method::GET,
            "/api/v1/entries?from=2025-01-01&to=2025-01-31",
            None,
            Some(&alice.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn two_factor_state_machine_replay_and_lazy_disable() {
    let app = TestApp::new().await;
    let password = "correct horse battery staple";
    app.register("carol", password).await;
    let session = app.session("carol", password).await;

    let (status, _, setup) = app
        .call(
            Method::GET,
            "/api/v1/auth/2fa/setup",
            None,
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let secret = setup["secret_base32"].as_str().unwrap().to_owned();
    assert!(setup["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/"));

    let enable_code = code_at_step(&secret, "carol", 0);
    let (status, _, value) = app
        .call(
            Method::POST,
            "/api/v1/auth/2fa/enable",
            Some(json!({ "code": enable_code })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");

    let (status, _, login) = app.login("carol", password).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["mfa_required"], true);
    assert!(login.get("access_token").is_none());
    let mfa_token = login["mfa_token"].as_str().unwrap().to_owned();

    let (status, _, value) = app
        .call(
            Method::GET,
            "/api/v1/auth/me",
            None,
            Some(&mfa_token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "mfa_required");

    let verify_code = code_at_step(&secret, "carol", 1);
    let (status, headers, value) = app
        .call(
            Method::POST,
            "/api/v1/auth/2fa/verify",
            Some(json!({ "mfa_token": mfa_token, "code": verify_code })),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let token = value["access_token"].as_str().unwrap().to_owned();
    let cookie = refresh_cookie(&headers);

    let (status, _, value) = app
        .call(
            Method::POST,
            "/api/v1/auth/2fa/verify",
            Some(json!({ "mfa_token": mfa_token, "code": verify_code })),
            None,
            Some(&cookie),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "code_already_used");

    // The disable endpoint is limited to 3/day/user.
    for attempt in 0..4 {
        let (status, _, value) = app
            .call(
                Method::POST,
                "/api/v1/auth/2fa/disable",
                Some(json!({ "password": password })),
                Some(&token),
                None,
                None,
            )
            .await;
        if attempt < 3 {
            assert_eq!(status, StatusCode::OK, "{value}");
        } else {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
            assert_eq!(value["error"]["code"], "rate_limited");
        }
    }

    // Move the lazy-disable deadline into the past and verify the next
    // authenticated request turns 2FA off and invalidates the token.
    let past = now_unix() - 1;
    sqlx::query("UPDATE users SET twofa_disable_at = ? WHERE username_norm = 'carol'")
        .bind(past)
        .execute(&app.state.pool)
        .await
        .unwrap();
    let (status, _, value) = app
        .call(
            Method::GET,
            "/api/v1/auth/me",
            None,
            Some(&token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    let (enabled, version): (bool, i64) =
        sqlx::query_as("SELECT totp_enabled, token_version FROM users WHERE username_norm = 'carol'")
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    assert!(!enabled);
    assert_eq!(version, 1);
}

#[tokio::test]
async fn export_and_import_are_scoped_idempotent_and_do_not_touch_other_dates() {
    let app = TestApp::new().await;
    let password = "correct horse battery staple";
    app.register("dave", password).await;
    let session = app.session("dave", password).await;

    let (status, _, tag) = app
        .call(
            Method::POST,
            "/api/v1/tags",
            Some(json!({ "name": "开心", "color": "#aaaaaa" })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let tag_id = tag["id"].as_i64().unwrap();

    app.call(
        Method::PUT,
        "/api/v1/entries/2025-02-01",
        Some(json!({ "content_md": "local", "tag_ids": [tag_id] })),
        Some(&session.token),
        None,
        None,
    )
    .await;
    app.call(
        Method::PUT,
        "/api/v1/entries/2025-02-03",
        Some(json!({ "content_md": "untouched", "tag_ids": [] })),
        Some(&session.token),
        None,
        None,
    )
    .await;

    let (status, headers, exported) = app
        .call(
            Method::POST,
            "/api/v1/export/prepare",
            Some(json!({ "scope": "all" })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(exported["format"], "diary");
    assert_eq!(exported["entries"].as_array().unwrap().len(), 2);

    let (status, _, only_one) = app
        .call(
            Method::POST,
            "/api/v1/export/prepare",
            Some(json!({ "scope": "dates", "dates": ["2025-02-01", "2025-02-01"] })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(only_one["entries"].as_array().unwrap().len(), 1);

    let import_body = json!({
        "tags": [
            { "name": "开心", "color": "#111111" },
            { "name": "新标签", "color": "#222222", "emoji": "✨" },
            { "name": "未使用标签", "color": "#333333" }
        ],
        "entries": [
            {
                "date": "2025-02-01",
                "content_md": "imported",
                "tags": ["开心", "新标签"],
                "created_at": "2020-01-01T00:00:00Z"
            },
            {
                "date": "2025-02-02",
                "content_md": "new day",
                "tags": []
            }
        ],
        "scope": "all"
    });
    let (status, _, preview) = app
        .call(
            Method::POST,
            "/api/v1/import/preview",
            Some(import_body.clone()),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["create"], 1);
    assert_eq!(preview["summary"]["update"], 1);
    assert_eq!(preview["summary"]["tags_create"], 1);
    let import_id = preview["import_id"].as_str().unwrap().to_owned();

    let (status, _, committed) = app
        .call(
            Method::POST,
            "/api/v1/import/commit",
            Some(json!({ "import_id": import_id, "scope": "all" })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["created"], 1);
    assert_eq!(committed["updated"], 1);
    assert_eq!(committed["tags_created"], 1);

    let (status, _, tags) = app
        .call(
            Method::GET,
            "/api/v1/tags?include_archived=true",
            None,
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let tags = tags.as_array().unwrap();
    let happy = tags.iter().find(|tag| tag["name"] == "开心").unwrap();
    assert_eq!(happy["color"], "#aaaaaa", "local tag attributes must win");
    assert!(tags.iter().any(|tag| tag["name"] == "新标签"));
    assert!(!tags.iter().any(|tag| tag["name"] == "未使用标签"));

    let (status, _, entries) = app
        .call(
            Method::GET,
            "/api/v1/entries?from=2025-02-01&to=2025-02-28",
            None,
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let entries = entries.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let imported = entries.iter().find(|entry| entry["date"] == "2025-02-01").unwrap();
    assert_eq!(imported["content_md"], "imported");
    assert_eq!(imported["tags"].as_array().unwrap().len(), 2);
    let untouched = entries.iter().find(|entry| entry["date"] == "2025-02-03").unwrap();
    assert_eq!(untouched["content_md"], "untouched");

    // Re-importing is idempotent.
    let (_, _, preview) = app
        .call(
            Method::POST,
            "/api/v1/import/preview",
            Some(import_body),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(preview["summary"]["create"], 0);
    assert_eq!(preview["summary"]["update"], 2);
    assert_eq!(preview["summary"]["tags_create"], 0);
    let import_id = preview["import_id"].as_str().unwrap().to_owned();
    let (_, _, committed) = app
        .call(
            Method::POST,
            "/api/v1/import/commit",
            Some(json!({ "import_id": import_id, "scope": "all" })),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(committed["created"], 0);
    assert_eq!(committed["updated"], 2);
    assert_eq!(committed["tags_created"], 0);
}

#[tokio::test]
async fn origin_checks_registration_limits_and_body_limits() {
    let app = TestApp::new().await;
    let (status, _, value) = app
        .call(
            Method::GET,
            "/api/v1/healthz",
            None,
            None,
            None,
            Some("http://evil.example"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(value["error"]["code"], "invalid_origin");

    for index in 1..=6 {
        let (status, _) = app.register(&format!("user{index}"), "correct horse battery").await;
        if index < 6 {
            assert_eq!(status, StatusCode::CREATED, "registration {index}");
        } else {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let huge = "x".repeat(2 * 1024 * 1024 + 1);
    let (status, _, value) = app
        .call(
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({ "username": "user1", "password": huge })),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(value["error"]["code"], "payload_too_large");
}

#[tokio::test]
async fn import_preview_accepts_bodies_above_the_global_two_mib_limit() {
    let app = TestApp::new().await;
    let password = "correct horse battery staple";
    app.register("large", password).await;
    let session = app.session("large", password).await;

    let chunk = "x".repeat(900_000);
    let body = json!({
        "tags": [],
        "entries": [
            { "date": "2025-04-01", "content_md": chunk },
            { "date": "2025-04-02", "content_md": chunk },
            { "date": "2025-04-03", "content_md": chunk }
        ],
        "scope": "all"
    });
    assert!(serde_json::to_vec(&body).unwrap().len() > 2 * 1024 * 1024);

    let (status, _, preview) = app
        .call(
            Method::POST,
            "/api/v1/import/preview",
            Some(body),
            Some(&session.token),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["create"], 3);
}
