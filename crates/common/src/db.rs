use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::error::Result;

pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Runs embedded migrations (idempotent; safe to call from both binaries).
pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("../../migrations").run(pool).await?;
    Ok(())
}
