//! `worker` binary — claims verification jobs from the Postgres-backed queue
//! and runs the rebuild → byte-compare pipeline for each.

mod compare;
mod git;
mod logger;
mod pipeline;
mod sandbox;

use std::sync::Arc;
use std::time::Duration;

use soroban_verify_common::config::Config;
use soroban_verify_common::trust::TrustPolicy;
use soroban_verify_common::{db, repo};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tracing_subscriber::EnvFilter;

use pipeline::WorkerCtx;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cfg = Config::from_env()?;
    let pool = db::connect(&cfg.database_url).await?;
    db::migrate(&pool).await?;

    std::fs::create_dir_all(&cfg.build_scratch_dir)?;

    if !docker_available().await {
        tracing::warn!(
            "docker CLI not found or not responding — builds will fail until it is available"
        );
    }

    let poll_interval = Duration::from_millis(cfg.worker_poll_interval_ms);
    let semaphore = Arc::new(Semaphore::new(cfg.max_concurrent_builds));
    let ctx = Arc::new(WorkerCtx {
        pool: pool.clone(),
        trust: TrustPolicy::from_config(&cfg),
        cfg,
    });

    tracing::info!("soroban-verify worker started");
    loop {
        // Take a build slot *before* claiming, so claimed jobs never sit idle
        // in `running` while we wait for capacity.
        let permit = semaphore.clone().acquire_owned().await?;
        match repo::claim_next_job(&pool).await {
            Ok(Some(job)) => {
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let job_id = job.id;
                    tracing::info!(%job_id, contract = %job.contract_id, "claimed job");
                    pipeline::run(&ctx, job).await;
                    drop(permit);
                });
            }
            Ok(None) => {
                drop(permit);
                tokio::time::sleep(poll_interval).await;
            }
            Err(e) => {
                drop(permit);
                tracing::error!(error = %e, "failed to poll job queue");
                tokio::time::sleep(poll_interval).await;
            }
        }
    }
}

async fn docker_available() -> bool {
    Command::new("docker")
        .arg("version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}
