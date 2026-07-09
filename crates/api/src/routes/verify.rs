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
use soroban_verify_common::{repo, validate};
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

    // TODO(M2): confirm the contract exists on-chain (SorobanRpc) and pre-fill
    // build_config from embedded SEP-58 metadata before enqueueing.
    // TODO(M3): rate limiting / dedup of in-flight jobs per (contract, commit).

    let job = repo::insert_job(
        &st.pool,
        &NewJob {
            contract_id: req.contract_id,
            network: req.network,
            repo_url: req.repo_url,
            commit_sha: req.commit_sha,
            build_config: req.build_config,
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
