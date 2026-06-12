//! Shared application state passed to every handler.

use std::sync::Arc;

use sqlx::PgPool;

use crate::cache::Cache;
use crate::config::Config;

pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub cache: Cache,
}

impl AppState {
    pub fn new(config: Config, pool: PgPool, cache: Cache) -> Self {
        Self {
            config,
            pool,
            cache,
        }
    }
}

pub type SharedState = Arc<AppState>;
