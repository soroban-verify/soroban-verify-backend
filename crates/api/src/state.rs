use std::sync::Arc;

use soroban_verify_common::config::Config;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    /// Not read by any handler yet — needed once submission intake validates
    /// contracts on-chain via Soroban RPC (TODO(M2) in routes/verify.rs).
    #[allow(dead_code)]
    pub cfg: Arc<Config>,
}

impl AppState {
    pub fn new(pool: PgPool, cfg: Config) -> Self {
        Self {
            pool,
            cfg: Arc::new(cfg),
        }
    }
}
