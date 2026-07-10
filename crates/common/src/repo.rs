//! Database queries. The job queue is Postgres-backed (`FOR UPDATE SKIP
//! LOCKED`) so the MVP needs no extra queue infrastructure; workers scale
//! horizontally by just running more processes.

use sqlx::types::Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;
use crate::models::{
    BuildLogLine, JobStatus, Network, NewJob, NewVerification, TrustTier, Verification,
    VerificationJob,
};

pub async fn insert_job(pool: &PgPool, new: &NewJob) -> Result<VerificationJob> {
    let job = sqlx::query_as::<_, VerificationJob>(
        r#"
        INSERT INTO verification_jobs (contract_id, network, repo_url, commit_sha, build_config)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(&new.contract_id)
    .bind(new.network)
    .bind(&new.repo_url)
    .bind(&new.commit_sha)
    .bind(Json(&new.build_config))
    .fetch_one(pool)
    .await?;
    Ok(job)
}

pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<Option<VerificationJob>> {
    let job = sqlx::query_as::<_, VerificationJob>("SELECT * FROM verification_jobs WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(job)
}

/// Atomically claims the oldest queued job, marking it running. Concurrent
/// workers skip rows already locked by another claimant.
pub async fn claim_next_job(pool: &PgPool) -> Result<Option<VerificationJob>> {
    let job = sqlx::query_as::<_, VerificationJob>(
        r#"
        UPDATE verification_jobs
        SET status = 'running',
            started_at = now(),
            attempts = attempts + 1,
            updated_at = now()
        WHERE id = (
            SELECT id FROM verification_jobs
            WHERE status = 'queued'
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING *
        "#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(job)
}

pub async fn finish_job(
    pool: &PgPool,
    id: Uuid,
    status: JobStatus,
    trust_tier: Option<TrustTier>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE verification_jobs
        SET status = $2, trust_tier = $3, error = $4, finished_at = now(), updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(trust_tier)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn append_log_line(pool: &PgPool, job_id: Uuid, seq: i32, line: &str) -> Result<()> {
    sqlx::query("INSERT INTO build_log_lines (job_id, seq, line) VALUES ($1, $2, $3)")
        .bind(job_id)
        .bind(seq)
        .bind(line)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn log_lines_after(
    pool: &PgPool,
    job_id: Uuid,
    after_seq: i32,
) -> Result<Vec<BuildLogLine>> {
    let lines = sqlx::query_as::<_, BuildLogLine>(
        "SELECT * FROM build_log_lines WHERE job_id = $1 AND seq > $2 ORDER BY seq",
    )
    .bind(job_id)
    .bind(after_seq)
    .fetch_all(pool)
    .await?;
    Ok(lines)
}

/// Publishes (or replaces) the canonical verification record for a contract.
/// TODO(M4/M5): revocation flows and multi-verifier records will replace this
/// last-write-wins upsert.
pub async fn upsert_verification(pool: &PgPool, v: &NewVerification) -> Result<Verification> {
    let row = sqlx::query_as::<_, Verification>(
        r#"
        INSERT INTO verifications
            (job_id, contract_id, network, repo_url, commit_sha, wasm_hash,
             rebuilt_wasm_hash, image_digest, trust_tier, status, sep58_mismatch)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (contract_id, network) DO UPDATE SET
            job_id = EXCLUDED.job_id,
            repo_url = EXCLUDED.repo_url,
            commit_sha = EXCLUDED.commit_sha,
            wasm_hash = EXCLUDED.wasm_hash,
            rebuilt_wasm_hash = EXCLUDED.rebuilt_wasm_hash,
            image_digest = EXCLUDED.image_digest,
            trust_tier = EXCLUDED.trust_tier,
            status = EXCLUDED.status,
            sep58_mismatch = EXCLUDED.sep58_mismatch,
            verified_at = now()
        RETURNING *
        "#,
    )
    .bind(v.job_id)
    .bind(&v.contract_id)
    .bind(v.network)
    .bind(&v.repo_url)
    .bind(&v.commit_sha)
    .bind(&v.wasm_hash)
    .bind(&v.rebuilt_wasm_hash)
    .bind(&v.image_digest)
    .bind(v.trust_tier)
    .bind(v.status)
    .bind(v.sep58_mismatch)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn verification_for_contract(
    pool: &PgPool,
    contract_id: &str,
    network: Network,
) -> Result<Option<Verification>> {
    let row = sqlx::query_as::<_, Verification>(
        "SELECT * FROM verifications WHERE contract_id = $1 AND network = $2",
    )
    .bind(contract_id)
    .bind(network)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Paginated explorer feed. `verified: Some(true)` returns only successful
/// verifications; `Some(false)` only mismatches; `None` everything.
pub async fn list_verifications(
    pool: &PgPool,
    verified: Option<bool>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Verification>, i64)> {
    let filter = match verified {
        Some(true) => "WHERE status = 'verified'",
        Some(false) => "WHERE status <> 'verified'",
        None => "",
    };

    let items = sqlx::query_as::<_, Verification>(&format!(
        "SELECT * FROM verifications {filter} ORDER BY verified_at DESC LIMIT $1 OFFSET $2"
    ))
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM verifications {filter}"))
        .fetch_one(pool)
        .await?;

    Ok((items, total))
}
