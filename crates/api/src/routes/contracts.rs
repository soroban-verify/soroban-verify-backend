//! Paginated explorer feed: `GET /v1/contracts?verified=true&page=1&per_page=20`.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use soroban_verify_common::repo;

use crate::error::ApiResult;
use crate::state::AppState;

const MAX_PER_PAGE: u32 = 100;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub verified: Option<bool>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

pub async fn list(
    State(st): State<AppState>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, MAX_PER_PAGE);
    let offset = i64::from(page - 1) * i64::from(per_page);

    let (items, total) =
        repo::list_verifications(&st.pool, params.verified, i64::from(per_page), offset).await?;

    Ok(Json(json!({
        "items": items,
        "page": page,
        "per_page": per_page,
        "total": total,
    })))
}
