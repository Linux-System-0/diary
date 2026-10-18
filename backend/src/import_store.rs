use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{format::DiaryPayload, util::now_unix};

pub const IMPORT_TTL_SECS: i64 = 10 * 60;

#[derive(Clone)]
struct StoredImport {
    id: String,
    created_at: i64,
    payload: DiaryPayload,
}

#[derive(Default)]
pub struct ImportStore {
    inner: Mutex<HashMap<i64, StoredImport>>,
}

impl ImportStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Only one preview is kept per user; a newer preview replaces the old one.
    pub async fn put(&self, user_id: i64, id: String, payload: DiaryPayload) {
        let mut inner = self.inner.lock().await;
        inner.insert(
            user_id,
            StoredImport {
                id,
                created_at: now_unix(),
                payload,
            },
        );
    }

    pub async fn get(&self, user_id: i64, import_id: &str) -> Option<DiaryPayload> {
        let mut inner = self.inner.lock().await;
        let Some(stored) = inner.get(&user_id) else {
            return None;
        };
        if stored.id != import_id {
            return None;
        }
        if now_unix() - stored.created_at > IMPORT_TTL_SECS {
            inner.remove(&user_id);
            return None;
        }
        Some(stored.payload.clone())
    }

    pub async fn remove(&self, user_id: i64) {
        self.inner.lock().await.remove(&user_id);
    }
}
