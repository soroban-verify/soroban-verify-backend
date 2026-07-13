use std::sync::Arc;

use soroban_verify_common::config::Config;
use soroban_verify_common::models::Network;
use soroban_verify_common::rpc::SorobanRpc;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cfg: Arc<Config>,
}

impl AppState {
    pub fn new(pool: PgPool, cfg: Config) -> Self {
        Self {
            pool,
            cfg: Arc::new(cfg),
        }
    }

    /// Returns a `SorobanRpc` client configured for the given network.
    pub fn rpc(&self, network: &Network) -> Result<SorobanRpc, soroban_verify_common::Error> {
        let url = self.cfg.rpc_url(*network)?;
        Ok(SorobanRpc::new(url))
    }
}
