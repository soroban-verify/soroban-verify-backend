//! Submission intake and job status/log streaming.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
use soroban_verify_common::models::{BuildConfig, Network, NewJob, VerificationJob};
use soroban_verify_common::{repo, sep58, validate, Error};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitRequest {
    pub contract_id: String,
    pub network: Network,
    pub repo_url: String,
    pub commit_sha: String,
    #[serde(default)]
    pub build_config: BuildConfig,
}

/// `POST /v1/verify` — enqueue a verification job.
pub async fn submit(
    State(st): State<AppState>,
    Json(req): Json<SubmitRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    validate::contract_id(&req.contract_id)?;
    validate::repo_url(&req.repo_url)?;
    validate::commit_sha(&req.commit_sha)?;
    if let Some(h) = &req.build_config.expected_wasm_hash {
        validate::wasm_hash(h)?;
    }

    // TODO(M3): rate limiting / dedup of in-flight jobs per (contract, commit).

    // ── Confirm contract exists on-chain ──────────────────────────────
    let rpc = st.rpc(&req.network)?;
    let _ = rpc.contract_wasm_hash(&req.contract_id).await.map_err(|e| {
        // Map "contract not found" into a 404 response; surface all other
        // errors as-is via the IntoResponse mapping (BAD_GATEWAY for
        // Rpc/Http errors, INTERNAL_SERVER_ERROR for everything else).
        let not_found = matches!(&e, Error::Rpc(msg) if msg.contains("contract not found on-chain"));
        if not_found {
            ApiError::not_found(format!(
                "contract {} not found on {}",
                req.contract_id, req.network,
            ))
        } else {
            ApiError(e)
        }
    })?;

    // ── Pre-fill build_config from SEP-58 metadata ───────────────────
    let mut build_config = req.build_config;

    // Fetch the on-chain Wasm bytes to extract embedded SEP-58 metadata.
    // Failing gracefully: if the fetch fails (e.g. wasm bytes not yet
    // indexed), we proceed with the user-supplied build_config alone.
    if let Ok(wasm_bytes) = rpc.fetch_contract_wasm(&req.contract_id).await {
        if let Some(meta) = sep58::resolve_from_wasm(&wasm_bytes) {
            // Pre-fill image from SEP-58 metadata if not explicitly provided.
            if build_config.image.is_none() {
                build_config.image = meta.build_image;
            }
            // Other SEP-58 fields (rust_version, soroban_cli_version) are
            // informational and not currently mapped to BuildConfig fields,
            // but the cross-check mechanism uses source_repo and commit_sha
            // downstream in the worker pipeline.
        }
    }

    let job = repo::insert_job(
        &st.pool,
        &NewJob {
            contract_id: req.contract_id,
            network: req.network,
            repo_url: req.repo_url,
            commit_sha: req.commit_sha,
            build_config,
        },
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job_id": job.id, "status": job.status })),
    ))
}

/// `GET /v1/verify/{job_id}` — job status.
pub async fn job_status(
    State(st): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<VerificationJob>> {
    let job = repo::get_job(&st.pool, job_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("job {job_id}")))?;
    Ok(Json(job))
}

/// `GET /v1/verify/{job_id}/logs` — live build log over SSE. Replays existing
/// lines, then follows until the job reaches a terminal state.
pub async fn job_logs_sse(
    State(st): State<AppState>,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    repo::get_job(&st.pool, job_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("job {job_id}")))?;

    let pool = st.pool.clone();
    let stream = async_stream::stream! {
        let mut after_seq = 0i32;
        loop {
            match repo::log_lines_after(&pool, job_id, after_seq).await {
                Ok(lines) => {
                    for l in lines {
                        after_seq = l.seq;
                        yield Ok(Event::default().event("log").data(l.line));
                    }
                }
                Err(e) => {
                    tracing::warn!(%job_id, error = %e, "log stream query failed");
                    break;
                }
            }

            match repo::get_job(&pool, job_id).await {
                Ok(Some(job)) if job.status.is_terminal() => {
                    yield Ok(Event::default().event("done").data(job.status.to_string()));
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
