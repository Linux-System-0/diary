use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{config::Config, import_store::ImportStore};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: SqlitePool,
    pub imports: Arc<ImportStore>,
}
