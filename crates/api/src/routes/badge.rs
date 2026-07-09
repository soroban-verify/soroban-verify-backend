//! Embeddable status badges: `GET /badge/{contract_id}.svg?network=mainnet`.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use soroban_verify_common::models::{Network, TrustTier, VerificationStatus};
use soroban_verify_common::{repo, validate};

use crate::error::ApiResult;
use crate::routes::verifications::NetworkParam;
use crate::state::AppState;

pub async fn badge(
    State(st): State<AppState>,
    Path(contract_id): Path<String>,
    Query(params): Query<NetworkParam>,
) -> ApiResult<Response> {
    // Routed as /badge/{contract_id} so the .svg suffix arrives as part of
    // the path parameter; strip it before validating.
    let contract_id = contract_id.trim_end_matches(".svg");
    validate::contract_id(contract_id)?;
    let network = params.network.unwrap_or(Network::Mainnet);

    let (message, color) =
        match repo::verification_for_contract(&st.pool, contract_id, network).await? {
            Some(v) => match (v.status, v.trust_tier) {
                (VerificationStatus::Verified, TrustTier::Trusted) => {
                    ("verified · trusted build", "#3fb950")
                }
                (VerificationStatus::Verified, TrustTier::Auditable) => {
                    ("verified · auditable build", "#d29922")
                }
                (VerificationStatus::Verified, TrustTier::DeployerSupplied) => {
                    ("verified · deployer-supplied build", "#db6d28")
                }
                (VerificationStatus::Mismatch, _) => ("mismatch", "#f85149"),
            },
            None => ("unverified", "#8b949e"),
        };

    let svg = render_badge("soroban verify", message, color);
    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "max-age=300, s-maxage=300"),
        ],
        svg,
    )
        .into_response())
}

/// Minimal shields.io-style flat badge.
fn render_badge(label: &str, message: &str, color: &str) -> String {
    const CHAR_W: usize = 7;
    const PAD: usize = 10;
    let label_w = label.len() * CHAR_W + PAD * 2;
    let msg_w = message.len() * CHAR_W + PAD * 2;
    let total_w = label_w + msg_w;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="20" role="img" aria-label="{label}: {message}">
  <rect width="{label_w}" height="20" fill="#555"/>
  <rect x="{label_w}" width="{msg_w}" height="20" fill="{color}"/>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="{label_mid}" y="14">{label}</text>
    <text x="{msg_mid}" y="14">{message}</text>
  </g>
</svg>"##,
        label_mid = label_w / 2,
        msg_mid = label_w + msg_w / 2,
    )
}

#[cfg(test)]
mod tests {
    use super::render_badge;

    #[test]
    fn badge_contains_message_and_color() {
        let svg = render_badge("soroban verify", "verified · trusted build", "#3fb950");
        assert!(svg.contains("verified · trusted build"));
        assert!(svg.contains("#3fb950"));
        assert!(svg.starts_with("<svg"));
    }
}
