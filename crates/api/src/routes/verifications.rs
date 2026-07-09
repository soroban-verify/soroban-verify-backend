//! Canonical verification records.

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use soroban_verify_common::models::{Network, Verification};
use soroban_verify_common::{repo, validate};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct NetworkParam {
    pub network: Option<Network>,
}

/// `GET /v1/verifications/{contract_id}?network=mainnet`
pub async fn get_one(
    State(st): State<AppState>,
    Path(contract_id): Path<String>,
    Query(params): Query<NetworkParam>,
) -> ApiResult<Json<Verification>> {
    validate::contract_id(&contract_id)?;
    let network = params.network.unwrap_or(Network::Mainnet);

    let v = repo::verification_for_contract(&st.pool, &contract_id, network)
        .await?
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "no verification record for {contract_id} on {network}"
            ))
        })?;
    Ok(Json(v))
}
