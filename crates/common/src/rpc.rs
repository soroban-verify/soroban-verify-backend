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

/// Subset of the `sendTransaction` response we care about. `status` is one
/// of `PENDING` / `DUPLICATE` / `TRY_AGAIN_LATER` / `ERROR` per the Soroban
/// RPC spec; `hash` is the on-chain transaction hash (hex). The full
/// `errorResultXdr` (base64 XDR) is preserved for diagnostics.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SendTransactionResult {
    pub status: String,
    pub hash: String,
    pub error_result_xdr: Option<String>,
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
    /// Uses `getLedgerEntries` to:
    ///   1. strkey-decode the contract id (stellar-strkey crate),
    ///   2. build the XDR `LedgerKey::ContractData` for the contract instance
    ///      (stellar-xdr crate), fetch it, and read the Wasm hash from the
    ///      `ContractExecutable`,
    ///   3. base64/XDR plumbing for the request/response envelopes.
    pub async fn contract_wasm_hash(&self, contract_id: &str) -> Result<String> {
        use stellar_xdr::{Limits, ReadXdr, WriteXdr};

        // Strkey-decode the contract ID to verify the checksum and get raw bytes
        let contract_strkey = stellar_strkey::Contract::from_string(contract_id)
            .map_err(|e| Error::Rpc(format!("invalid contract id strkey: {e}")))?;
        let contract_bytes: [u8; 32] = contract_strkey.0;

        // Build LedgerKey::ContractData for the contract instance
        let ledger_key = build_contract_data_key(contract_bytes);

        // Encode to base64 XDR
        let key_b64 = ledger_key
            .to_xdr_base64(Limits::none())
            .map_err(|e| Error::Rpc(format!("XDR encoding failed: {e}")))?;

        // Call getLedgerEntries
        let params = serde_json::json!({ "keys": [key_b64] });
        let response: serde_json::Value =
            self.call("getLedgerEntries", Some(params)).await?;

        // Extract first entry
        let entry = response["entries"]
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| Error::Rpc("contract not found on-chain".into()))?;

        let entry_xdr = entry["xdr"]
            .as_str()
            .ok_or_else(|| Error::Rpc("missing xdr field in response entry".into()))?;

        // Decode the LedgerEntry XDR
        let ledger_entry =
            stellar_xdr::LedgerEntry::from_xdr_base64(entry_xdr, Limits::none())
                .map_err(|e| Error::Rpc(format!("XDR decoding failed: {e}")))?;

        // Extract Wasm hash from the ContractExecutable inside the contract instance
        match &ledger_entry.data {
            stellar_xdr::LedgerEntryData::ContractData(data) => {
                match &data.val {
                    stellar_xdr::ScVal::ContractInstance(instance) => {
                        match &instance.executable {
                            stellar_xdr::ContractExecutable::Wasm(hash) => {
                                Ok(hex::encode(hash.0))
                            }
                            other => Err(Error::Rpc(format!(
                                "contract executable is not Wasm: {other:?}"
                            ))),
                        }
                    }
                    other => Err(Error::Rpc(format!(
                        "unexpected contract data value type: {other:?}"
                    ))),
                }
            }
            other => Err(Error::Rpc(format!(
                "unexpected ledger entry type (expected ContractData): {other:?}"
            ))),
        }
    }

    /// Fetches the compiled Wasm bytes for `contract_id` from the network.
    ///
    /// Uses `getLedgerEntries` to:
    ///   1. strkey-decode the contract id (stellar-strkey crate),
    ///   2. build the XDR `LedgerKey::ContractCode` for the contract's Wasm,
    ///   3. fetch + base64/XDR-decode the `ContractCodeEntry` and return
    ///      its raw Wasm bytes.
    pub async fn fetch_contract_wasm(&self, contract_id: &str) -> Result<Vec<u8>> {
        use stellar_xdr::{Limits, ReadXdr, WriteXdr};

        // First resolve the Wasm hash to get the code key
        let wasm_hash_hex = self.contract_wasm_hash(contract_id).await?;
        let wasm_hash_bytes = hex::decode(&wasm_hash_hex)
            .map_err(|e| Error::Rpc(format!("invalid wasm hash hex: {e}")))?;

        let hash_arr: [u8; 32] = wasm_hash_bytes.as_slice().try_into()
            .map_err(|_| Error::Rpc("wasm hash is not 32 bytes".into()))?;

        // Build LedgerKey::ContractCode for the Wasm code entry
        let ledger_key = build_contract_code_key(hash_arr);

        // Encode to base64 XDR
        let key_b64 = ledger_key
            .to_xdr_base64(Limits::none())
            .map_err(|e| Error::Rpc(format!("XDR encoding failed: {e}")))?;

        // Call getLedgerEntries
        let params = serde_json::json!({ "keys": [key_b64] });
        let response: serde_json::Value =
            self.call("getLedgerEntries", Some(params)).await?;

        // Extract entry
        let entry = response["entries"]
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| Error::Rpc("contract code not found on-chain".into()))?;

        let entry_xdr = entry["xdr"]
            .as_str()
            .ok_or_else(|| Error::Rpc("missing xdr field in response entry".into()))?;

        // Decode the LedgerEntry XDR
        let ledger_entry =
            stellar_xdr::LedgerEntry::from_xdr_base64(entry_xdr, Limits::none())
                .map_err(|e| Error::Rpc(format!("XDR decoding failed: {e}")))?;

        // Extract Wasm bytes from the ContractCodeEntry
        match &ledger_entry.data {
            stellar_xdr::LedgerEntryData::ContractCode(code_entry) => {
                Ok(code_entry.code.clone().into_vec())
            }
            other => Err(Error::Rpc(format!(
                "unexpected ledger entry type (expected ContractCode): {other:?}"
            ))),
        }
    }

    /// Submits a signed transaction envelope (base64 XDR) to the Soroban
    /// network via the `sendTransaction` RPC method. The envelope must be a
    /// fully-signed `TransactionEnvelope`; this method only handles transport.
    ///
    /// Used by the M3 on-chain attestation step to submit the `attest` call
    /// to the verification registry contract.
    pub async fn send_transaction(&self, tx_envelope_xdr: &str) -> Result<SendTransactionResult> {
        let params = serde_json::json!({ "transaction": tx_envelope_xdr });
        self.call("sendTransaction", Some(params)).await
    }
}

/// Builds a `LedgerKey::ContractData` XDR value to look up the contract
/// instance (which contains the Wasm executable hash) for `contract_bytes`.
fn build_contract_data_key(contract_bytes: [u8; 32]) -> stellar_xdr::LedgerKey {
    use stellar_xdr::{
        ContractDataDurability, ContractId, Hash, LedgerKey, LedgerKeyContractData, ScAddress,
        ScVal,
    };

    LedgerKey::ContractData(LedgerKeyContractData {
        contract: ScAddress::Contract(ContractId(Hash(contract_bytes))),
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    })
}

/// Builds a `LedgerKey::ContractCode` XDR value to look up the contract
/// Wasm bytes for `wasm_hash_bytes`.
fn build_contract_code_key(wasm_hash_bytes: [u8; 32]) -> stellar_xdr::LedgerKey {
    use stellar_xdr::{Hash, LedgerKey, LedgerKeyContractCode};

    LedgerKey::ContractCode(LedgerKeyContractCode {
        hash: Hash(wasm_hash_bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::{Limits, ReadXdr, WriteXdr};

    #[test]
    fn test_build_contract_data_key_is_valid_xdr() {
        let bytes = [0u8; 32];
        let key = build_contract_data_key(bytes);
        let b64 = key.to_xdr_base64(Limits::none()).expect("should encode");
        // Verify it round-trips
        let decoded =
            stellar_xdr::LedgerKey::from_xdr_base64(&b64, Limits::none()).expect("should decode");
        match decoded {
            stellar_xdr::LedgerKey::ContractData(_) => {}
            _ => panic!("unexpected key type"),
        }
    }

    #[test]
    fn test_build_contract_code_key_is_valid_xdr() {
        let bytes = [0u8; 32];
        let key = build_contract_code_key(bytes);
        let b64 = key.to_xdr_base64(Limits::none()).expect("should encode");
        let decoded =
            stellar_xdr::LedgerKey::from_xdr_base64(&b64, Limits::none()).expect("should decode");
        match decoded {
            stellar_xdr::LedgerKey::ContractCode(_) => {}
            _ => panic!("unexpected key type"),
        }
    }

    #[test]
    fn test_contract_wasm_hash_errors_on_invalid_strkey() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let rpc = SorobanRpc::new("https://example.com/rpc");
        let result = rt.block_on(rpc.contract_wasm_hash("not-a-valid-strkey"));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("invalid contract id"),
            "expected invalid contract id error, got: {err}"
        );
    }

    #[test]
    fn test_contract_wasm_hash_connection_error() {
        // Test that the RPC call flow produces an appropriate error when
        // the RPC server is unreachable.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        let rpc = SorobanRpc::new("http://127.0.0.1:1");
        let result = rt.block_on(rpc.contract_wasm_hash(
            "CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE",
        ));
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("error sending request") || err.contains("connect"),
            "expected connection error, got: {err}"
        );
    }
}
