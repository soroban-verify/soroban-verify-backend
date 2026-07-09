//! `api` binary — the public REST/SSE surface of soroban-verify.

mod error;
mod routes;
mod state;

use soroban_verify_common::{config::Config, db};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env()?;
    let pool = db::connect(&cfg.database_url).await?;
    db::migrate(&pool).await?;

    let addr = cfg.api_bind_addr;
    let app = routes::router(state::AppState::new(pool, cfg));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("soroban-verify api listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
