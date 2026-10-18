use axum::http::{HeaderMap, HeaderValue, header};

use crate::config::Config;

pub const REFRESH_COOKIE: &str = "diary_refresh";
pub const REFRESH_COOKIE_PATH: &str = "/api/v1/auth";

pub fn refresh_cookie(config: &Config, token: &str, max_age: u64) -> HeaderValue {
    let secure = if config.secure_cookie { "; Secure" } else { "" };
    let value = format!(
        "{REFRESH_COOKIE}={token}; Path={REFRESH_COOKIE_PATH}; HttpOnly; SameSite=Strict; \
         Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&value).expect("cookie 值仅含合法字符")
}

pub fn clear_refresh_cookie(config: &Config) -> HeaderValue {
    refresh_cookie(config, "", 0)
}

pub fn read_refresh_cookie(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == REFRESH_COOKIE && !value.is_empty()).then(|| value.to_owned())
    })
}
