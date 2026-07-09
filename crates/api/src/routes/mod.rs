mod badge;
mod contracts;
mod verifications;
mod verify;

use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/verify", post(verify::submit))
        .route("/v1/verify/{job_id}", get(verify::job_status))
        .route("/v1/verify/{job_id}/logs", get(verify::job_logs_sse))
        .route(
            "/v1/verifications/{contract_id}",
            get(verifications::get_one),
        )
        .route("/v1/contracts", get(contracts::list))
        .route("/badge/{contract_id}", get(badge::badge))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
