use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use secrecy::SecretBox;

/// Runtime configuration, loaded once from the environment (optionally `.env`).
///
/// Secrets are kept in `SecretBox` so they are zeroized on drop and never leaked
/// through `Debug`.
pub struct Config {
    pub bind: SocketAddr,
    pub db_path: PathBuf,
    pub jwt_secret: SecretBox<Vec<u8>>,
    pub master_key: SecretBox<[u8; 32]>,
    pub cors_origins: Vec<String>,
    pub allow_registration: bool,
    pub secure_cookie: bool,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
    pub mfa_ttl: Duration,
    pub totp_skew: u8,
    pub twofa_disable_delay: Duration,
    pub max_entry_bytes: usize,
    pub max_export_bytes: usize,
    pub max_import_bytes: usize,
    pub max_tags: i64,
    pub max_tags_per_entry: usize,
    pub rust_log: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind = env_or("DIARY_BIND", "127.0.0.1:8787")
            .parse()
            .context("DIARY_BIND 不是合法的监听地址")?;

        let db_path = required("DIARY_DB_PATH")?.into();

        let jwt_secret = decode_secret("DIARY_JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            bail!("DIARY_JWT_SECRET 必须至少 32 字节（base64 解码后）");
        }

        let master_raw = decode_secret("DIARY_MASTER_KEY")?;
        let master_key: [u8; 32] = master_raw
            .try_into()
            .map_err(|_| anyhow::anyhow!("DIARY_MASTER_KEY 必须是 32 字节（base64 解码后）"))?;

        let cors_origins = required("DIARY_CORS_ORIGINS")?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if cors_origins.is_empty() {
            bail!("DIARY_CORS_ORIGINS 不能为空");
        }
        for origin in &cors_origins {
            if origin == "*" || origin.ends_with('/') {
                bail!("DIARY_CORS_ORIGINS 必须是精确 origin，且不能使用 * 或结尾斜杠");
            }
        }

        Ok(Self {
            bind,
            db_path,
            jwt_secret: SecretBox::new(Box::new(jwt_secret)),
            master_key: SecretBox::new(Box::new(master_key)),
            cors_origins,
            allow_registration: bool_env("DIARY_ALLOW_REGISTRATION", true)?,
            secure_cookie: bool_env("DIARY_SECURE_COOKIE", true)?,
            access_ttl: secs_env("DIARY_ACCESS_TTL_SECS", 900)?,
            refresh_ttl: secs_env("DIARY_REFRESH_TTL_SECS", 2_592_000)?,
            mfa_ttl: secs_env("DIARY_MFA_TOKEN_TTL_SECS", 300)?,
            totp_skew: num_env("DIARY_TOTP_SKEW", 1)?,
            twofa_disable_delay: Duration::from_secs(
                num_env::<u64>("DIARY_TWOFA_DISABLE_DELAY_DAYS", 7)?
                    .saturating_mul(24 * 60 * 60),
            ),
            max_entry_bytes: num_env("DIARY_MAX_ENTRY_BYTES", 1_048_576)?,
            max_export_bytes: num_env("DIARY_MAX_EXPORT_BYTES", 67_108_864)?,
            max_import_bytes: num_env("DIARY_MAX_IMPORT_BYTES", 67_108_864)?,
            max_tags: num_env("DIARY_MAX_TAGS", 200)?,
            max_tags_per_entry: num_env("DIARY_MAX_TAGS_PER_ENTRY", 20)?,
            rust_log: env_or("DIARY_RUST_LOG", "info"),
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn required(key: &str) -> Result<String> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => bail!("缺少必填环境变量 {key}"),
    }
}

fn decode_secret(key: &str) -> Result<Vec<u8>> {
    let raw = required(key)?;
    STANDARD
        .decode(raw.trim())
        .with_context(|| format!("{key} 不是合法的 base64"))
}

fn bool_env(key: &str, default: bool) -> Result<bool> {
    match env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => bail!("{key} 必须是 true/false"),
        },
        Err(_) => Ok(default),
    }
}

fn num_env<T>(key: &str, default: T) -> Result<T>
where
    T: std::str::FromStr + Copy,
    T::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("{key} 不是合法数字: {e}")),
        Err(_) => Ok(default),
    }
}

fn secs_env(key: &str, default: u64) -> Result<Duration> {
    let secs: u64 = num_env(key, default)?;
    if secs == 0 {
        bail!("{key} 必须大于 0");
    }
    Ok(Duration::from_secs(secs))
}
