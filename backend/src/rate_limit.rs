use std::collections::HashMap;

use tokio::sync::Mutex;

use crate::util::now_unix;

#[derive(Default)]
struct Window {
    start: i64,
    count: u32,
    locked_until: i64,
}

#[derive(Default)]
pub struct RateLimits {
    inner: Mutex<HashMap<String, Window>>,
}

impl RateLimits {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remaining seconds if the key is currently locked.
    pub async fn locked_for(&self, key: &str, now: i64) -> Option<u64> {
        let inner = self.inner.lock().await;
        inner
            .get(key)
            .filter(|window| window.locked_until > now)
            .map(|window| (window.locked_until - now) as u64)
    }

    /// Records a failure. Once `limit` failures happen inside `window_secs`, the
    /// key is blocked for `lock_secs` (or until the window ends when zero).
    pub async fn record_failure(
        &self,
        key: &str,
        limit: u32,
        window_secs: i64,
        lock_secs: i64,
        now: i64,
    ) {
        let mut inner = self.inner.lock().await;
        prune(&mut inner, now);
        let window = inner.entry(key.to_owned()).or_default();
        if window.start == 0 || now - window.start >= window_secs {
            window.start = now;
            window.count = 0;
            window.locked_until = 0;
        }
        window.count += 1;
        if window.count >= limit {
            window.locked_until = if lock_secs > 0 {
                now + lock_secs
            } else {
                window.start + window_secs
            };
        }
    }

    pub async fn clear(&self, key: &str) {
        self.inner.lock().await.remove(key);
    }

    /// Counts an attempt against a fixed window. Returns the retry delay when the
    /// quota is already exhausted (the attempt is not counted).
    pub async fn check_and_hit(
        &self,
        key: &str,
        limit: u32,
        window_secs: i64,
        now: i64,
    ) -> Option<u64> {
        let mut inner = self.inner.lock().await;
        prune(&mut inner, now);
        let window = inner.entry(key.to_owned()).or_default();
        if window.start == 0 || now - window.start >= window_secs {
            window.start = now;
            window.count = 0;
            window.locked_until = 0;
        }
        if window.count >= limit {
            return Some((window.start + window_secs - now).max(1) as u64);
        }
        window.count += 1;
        None
    }
}

fn prune(inner: &mut HashMap<String, Window>, now: i64) {
    if inner.len() < 4096 {
        return;
    }
    inner.retain(|_, window| window.locked_until > now || now - window.start < 24 * 60 * 60);
}

pub fn login_key(username_norm: &str) -> String {
    format!("login:{username_norm}")
}

pub fn twofa_key(user_id: i64) -> String {
    format!("2fa:{user_id}")
}

pub fn disable_key(user_id: i64) -> String {
    format!("disable2fa:{user_id}")
}

pub fn write_key(user_id: i64) -> String {
    format!("write:{user_id}")
}

pub fn now() -> i64 {
    now_unix()
}
