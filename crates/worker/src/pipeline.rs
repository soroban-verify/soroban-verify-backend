//! The verification pipeline: resolve on-chain hash → clone pinned source →
//! sandboxed rebuild → byte-compare → publish result.

use std::time::Duration;

use soroban_verify_common::config::Config;
use soroban_verify_common::models::{
    JobStatus, NewVerification, TrustTier, Verification, VerificationJob, VerificationStatus,
};
use soroban_verify_common::rpc::SorobanRpc;
use soroban_verify_common::trust::TrustPolicy;
use soroban_verify_common::{repo, Result};
use sqlx::PgPool;

use crate::attest::{self, AttestConfig, AttestationOutcome};
use crate::logger::BuildLog;
use crate::{compare, git, sandbox};

pub struct WorkerCtx {
    pub pool: PgPool,
    pub cfg: Config,
    pub trust: TrustPolicy,
}

struct Outcome {
    status: VerificationStatus,
    tier: TrustTier,
    onchain_hash: String,
    rebuilt_hash: String,
    image: String,
}

/// Runs one claimed job to completion, always leaving it in a terminal state.
pub async fn run(ctx: &WorkerCtx, job: VerificationJob) {
    let mut log = BuildLog::new(ctx.pool.clone(), job.id);
    log.line(format!(
        "verification started: {} on {} from {} @ {}",
        job.contract_id, job.network, job.repo_url, job.commit_sha
    ))
    .await;

    let timeout = Duration::from_secs(ctx.cfg.build_timeout_secs);
    let result = tokio::time::timeout(timeout, execute(ctx, &job, &mut log)).await;

    let finished = match result {
        Err(_) => fail(ctx, &job, &mut log, "build timed out".into()).await,
        Ok(Err(e)) => fail(ctx, &job, &mut log, e.to_string()).await,
        Ok(Ok(outcome)) => publish(ctx, &job, &mut log, outcome).await,
    };

    if let Err(e) = finished {
        tracing::error!(job_id = %job.id, error = %e, "failed to record job outcome");
    }
}

async fn execute(ctx: &WorkerCtx, job: &VerificationJob, log: &mut BuildLog) -> Result<Outcome> {
    log.line("resolving on-chain wasm hash").await;
    let onchain_hash = onchain_wasm_hash(ctx, job).await?;
    log.line(format!("on-chain wasm hash: {onchain_hash}"))
        .await;

    log.line(format!("cloning {} @ {}", job.repo_url, job.commit_sha))
        .await;
    let workdir = tempfile::Builder::new()
        .prefix("sv-build-")
        .tempdir_in(&ctx.cfg.build_scratch_dir)?;
    let src = git::clone_at_commit(&job.repo_url, &job.commit_sha, workdir.path()).await?;

    // TODO(M2): read SEP-58 metadata from the on-chain Wasm and cross-check
    // it against the submitted repo/commit (soroban_verify_common::sep58).

    let build_config = &job.build_config.0;
    let image = build_config
        .image
        .clone()
        .unwrap_or_else(|| ctx.cfg.default_build_image.clone());
    let tier = ctx.trust.classify(&image);
    log.line(format!("build image: {image} (trust tier: {tier})"))
        .await;

    let wasm = sandbox::build(&src, &image, build_config, log).await?;
    let rebuilt_hash = compare::sha256_hex(&wasm);
    log.line(format!("rebuilt wasm hash: {rebuilt_hash}")).await;

    let status = if compare::hashes_match(&rebuilt_hash, &onchain_hash) {
        VerificationStatus::Verified
    } else {
        VerificationStatus::Mismatch
    };

    Ok(Outcome {
        status,
        tier,
        onchain_hash,
        rebuilt_hash,
        image,
    })
}

/// The hash the rebuild must reproduce. Until getLedgerEntries + XDR decoding
/// lands (see `SorobanRpc::contract_wasm_hash`), submissions may pin the
/// expected hash explicitly via `build_config.expected_wasm_hash`.
async fn onchain_wasm_hash(ctx: &WorkerCtx, job: &VerificationJob) -> Result<String> {
    if let Some(expected) = &job.build_config.0.expected_wasm_hash {
        return Ok(expected.to_lowercase());
    }
    let rpc = SorobanRpc::new(ctx.cfg.rpc_url(job.network)?);
    rpc.contract_wasm_hash(&job.contract_id).await
}

async fn publish(
    ctx: &WorkerCtx,
    job: &VerificationJob,
    log: &mut BuildLog,
    outcome: Outcome,
) -> Result<()> {
    let verification = repo::upsert_verification(
        &ctx.pool,
        &NewVerification {
            job_id: job.id,
            contract_id: job.contract_id.clone(),
            network: job.network,
            repo_url: job.repo_url.clone(),
            commit_sha: job.commit_sha.clone(),
            wasm_hash: outcome.onchain_hash,
            rebuilt_wasm_hash: Some(outcome.rebuilt_hash),
            image_digest: Some(outcome.image),
            trust_tier: outcome.tier,
            status: outcome.status,
            attestation_tx_hash: None,
            attester_address: None,
        },
    )
    .await?;

    // M3 on-chain attestation. Non-fatal: any failure is logged but does
    // not change the local verification status.
    submit_onchain_attestation(ctx, job, &verification, log).await;

    // TODO(M4): verify SEP-55 signed CI attestations when supplied and record
    // the strengthened provenance on the verification.

    let job_status = match outcome.status {
        VerificationStatus::Verified => JobStatus::Verified,
        VerificationStatus::Mismatch => JobStatus::Mismatch,
    };
    repo::finish_job(&ctx.pool, job.id, job_status, Some(outcome.tier), None).await?;
    log.line(format!("verification finished: {job_status}"))
        .await;
    Ok(())
}

async fn fail(
    ctx: &WorkerCtx,
    job: &VerificationJob,
    log: &mut BuildLog,
    reason: String,
) -> Result<()> {
    log.line(format!("verification failed: {reason}")).await;
    repo::finish_job(&ctx.pool, job.id, JobStatus::Failed, None, Some(&reason)).await?;
    Ok(())
}

/// Resolves the M3 attestation config (or `None` if M3 is disabled) and
/// submits the `attest` transaction. Records the outcome on the
/// `verifications` row when submission succeeds. All paths are best-effort:
/// the function never returns an error, and any failure is logged.
async fn submit_onchain_attestation(
    ctx: &WorkerCtx,
    job: &VerificationJob,
    verification: &Verification,
    log: &mut BuildLog,
) {
    let cfg = match (
        ctx.cfg.registry_contract_id.as_deref(),
        ctx.cfg.attester_secret_key.as_deref(),
    ) {
        (Some(registry_contract_id), Some(attester_secret_key)) => AttestConfig {
            registry_contract_id: registry_contract_id.to_string(),
            attester_secret_key: attester_secret_key.to_string(),
        },
        _ => {
            log.line(
                "m3 on-chain attestation skipped (REGISTRY_CONTRACT_ID \
                      and/or ATTESTER_SECRET_KEY not set)",
            )
            .await;
            return;
        }
    };

    let rpc_url = match ctx.cfg.rpc_url(job.network) {
        Ok(u) => u.to_string(),
        Err(e) => {
            log.line(format!(
                "m3 on-chain attestation skipped (no rpc url for {}): {e}",
                job.network
            ))
            .await;
            return;
        }
    };
    let rpc = SorobanRpc::new(rpc_url);

    match attest::submit_attestation(verification, Some(&cfg), &rpc).await {
        AttestationOutcome::Skipped => {}
        AttestationOutcome::Submitted {
            tx_hash,
            attester_address,
        } => {
            log.line(format!(
                "m3 on-chain attestation submitted (attester: {attester_address}, tx: {tx_hash})"
            ))
            .await;
            if let Err(e) = repo::update_attestation(
                &ctx.pool,
                &job.contract_id,
                job.network,
                &tx_hash,
                &attester_address,
            )
            .await
            {
                // Best-effort DB write; a failure here does not invalidate
                // the on-chain attestation that already happened.
                log.line(format!(
                    "m3 on-chain attestation recorded on-chain but failed to update db: {e}"
                ))
                .await;
            }
        }
        AttestationOutcome::Failed {
            attester_address,
            error,
        } => {
            log.line(format!(
                "m3 on-chain attestation failed (attester: {attester_address}): {error}"
            ))
            .await;
        }
    }
}
