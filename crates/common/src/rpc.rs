//! Minimal Soroban RPC (JSON-RPC 2.0 over HTTP) client.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub struct SorobanRpc {
    http: reqwest::Client,
    url: String,
}

#[derive(Serialize)]
struct RpcRequest<'a, P: Serialize> {
    jsonrpc: &'static str,
    id: u32,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<P>,
}

#[derive(Deserialize)]
struct RpcResponse<R> {
    result: Option<R>,
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LatestLedger {
    pub sequence: u32,
    pub protocol_version: u32,
}

impl SorobanRpc {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            url: url.into(),
        }
    }

    async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> Result<R> {
        let body = RpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let resp: RpcResponse<R> = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if let Some(err) = resp.error {
            return Err(Error::Rpc(format!(
                "{method}: {} ({})",
                err.message, err.code
            )));
        }
        resp.result
            .ok_or_else(|| Error::Rpc(format!("{method}: empty result")))
    }

    /// Connectivity / health probe.
    pub async fn latest_ledger(&self) -> Result<LatestLedger> {
        self.call::<(), _>("getLatestLedger", None).await
    }

    /// Resolves the sha256 hash of the Wasm installed for `contract_id`.
    ///
    /// TODO(M2): implement via `getLedgerEntries`:
    ///   1. strkey-decode the contract id (stellar-strkey crate),
    ///   2. build the XDR `LedgerKey::ContractData` for the contract instance
    ///      (stellar-xdr crate), fetch it, and read the Wasm hash from the
    ///      `ScContractInstance` executable,
    ///   3. base64/XDR plumbing for the request/response envelopes.
    ///
    /// Until then, submissions may carry `build_config.expected_wasm_hash`
    /// (dev/test only).
    pub async fn contract_wasm_hash(&self, contract_id: &str) -> Result<String> {
        let _ = contract_id;
        Err(Error::Rpc(
            "on-chain wasm hash resolution is not implemented yet (M2: \
             getLedgerEntries + XDR decode)"
                .into(),
        ))
    }

    /// Fetches the full Wasm bytes installed for `contract_id`.
    ///
    /// Used by the SEP-58 metadata cross-check (`crate::sep58::resolve_from_wasm`):
    /// once the bytes are in hand we can iterate the Wasm's `contractmetav0`
    /// custom section and compare the embedded `source_repo` / `commit_sha`
    /// against the submitter's claim.
    ///
    /// TODO(M2): implement via the same `getLedgerEntries` XDR plumbing as
    /// `contract_wasm_hash` — once the contract instance is decoded, the
    /// `ScContractInstance::executable` variant is `Wasm` and carries the
    /// Wasm hash; the actual bytes then need to be fetched via a second
    /// ledger entry (`ContractCodeEntry`) keyed by that hash. Until that
    /// lands, callers must treat the cross-check as "unknown" rather than
    /// as "mismatch".
    pub async fn fetch_contract_wasm(&self, contract_id: &str) -> Result<Vec<u8>> {
        let _ = contract_id;
        Err(Error::Rpc(
            "on-chain wasm bytes fetch is not implemented yet (M2: \
             getLedgerEntries + ContractCodeEntry)"
                .into(),
        ))
    }
}
