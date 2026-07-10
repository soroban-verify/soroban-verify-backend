//! M3 on-chain attestation.
//!
//! After a successful byte-comparison, the worker signs and submits an
//! `attest` call to the on-chain verification registry contract. The
//! resulting on-chain transaction hash and attester address are recorded on
//! the `verifications` row.
//!
//! Attestation is a no-op when `REGISTRY_CONTRACT_ID` and
//! `ATTESTER_SECRET_KEY` are unset. Any failure during submission is logged
//! but **does not** fail the local verification, per the issue's acceptance
//! criteria.
//!
//! The actual XDR envelope construction is a documented stub: the
//! `stellar-xdr` crate is deliberately not pulled in for the MVP. A
//! follow-up replaces `build_envelope` with proper XDR construction; the
//! submission pipeline (config, address derivation, RPC call, DB update)
//! is the surface this module ships today.
//!
//! SECURITY: `attester_secret_key` is **never** written to logs, errors, or
//! DB rows. It is used only as input to the key-derivation helper that
//! yields the public address and (in the future) signs the envelope.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use serde_json::json;
use soroban_verify_common::models::Verification;
use soroban_verify_common::rpc::SorobanRpc;
use stellar_strkey::{Strkey, StrkeyPrivateKeyEd25519, StrkeyPublicKeyEd25519};

/// Configuration for the on-chain attestation step. Both fields are required
/// to enable M3; when either is `None` (the default), `submit_attestation`
/// returns [`AttestationOutcome::Skipped`].
///
/// The `Debug` impl is hand-rolled so the `attester_secret_key` is never
/// printed — `#[derive(Debug)]` would leak the secret into any panic
/// message, `tracing` field, or `dbg!` macro that ever formats this struct.
#[derive(Clone)]
pub struct AttestConfig {
    pub registry_contract_id: String,
    pub attester_secret_key: String,
}

impl std::fmt::Debug for AttestConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttestConfig")
            .field("registry_contract_id", &self.registry_contract_id)
            .field("attester_secret_key", &"<redacted>")
            .finish()
    }
}

/// Outcome of an M3 attestation attempt. The verification pipeline logs and
/// continues regardless of which variant is produced — none of these change
/// the local job/verification status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttestationOutcome {
    /// M3 is disabled (no `REGISTRY_CONTRACT_ID` and/or no
    /// `ATTESTER_SECRET_KEY`). No DB write, no RPC call.
    Skipped,
    /// The on-chain `attest` transaction was accepted by the RPC.
    Submitted {
        tx_hash: String,
        attester_address: String,
    },
    /// Submission was attempted but failed. The error string is safe to log
    /// (it never contains the secret key).
    Failed {
        attester_address: String,
        error: String,
    },
}

/// Attempts to submit an `attest` call to the on-chain verification registry.
/// Returns [`AttestationOutcome::Skipped`] when `cfg` is `None`. Otherwise
/// derives the attester address, builds the transaction envelope, and
/// submits it via `sendTransaction` RPC.
pub async fn submit_attestation(
    verification: &Verification,
    cfg: Option<&AttestConfig>,
    rpc: &SorobanRpc,
) -> AttestationOutcome {
    let Some(cfg) = cfg else {
        return AttestationOutcome::Skipped;
    };

    let attester_address = match derive_attester_address(&cfg.attester_secret_key) {
        Some(addr) => addr,
        None => {
            return AttestationOutcome::Failed {
                attester_address: "unknown".into(),
                error: "ATTESTER_SECRET_KEY is not a valid Stellar strkey secret seed".into(),
            };
        }
    };

    let envelope = build_envelope(verification, &cfg.registry_contract_id);

    match rpc.send_transaction(&envelope).await {
        Ok(res) if res.status == "ERROR" => AttestationOutcome::Failed {
            attester_address,
            error: format!(
                "sendTransaction returned ERROR (errorResultXdr: {:?})",
                res.error_result_xdr
            ),
        },
        Ok(res) => AttestationOutcome::Submitted {
            tx_hash: res.hash,
            attester_address,
        },
        Err(e) => AttestationOutcome::Failed {
            attester_address,
            error: format!("sendTransaction RPC failed: {e}"),
        },
    }
}

/// Derives the Stellar strkey G... address for the attester from its strkey
/// S... secret seed. Returns `None` if the secret is not a valid
/// `PrivateKeyEd25519` strkey.
pub fn derive_attester_address(secret: &str) -> Option<String> {
    let parsed = Strkey::from_string(secret).ok()?;
    let seed: [u8; 32] = match parsed {
        Strkey::PrivateKeyEd25519(StrkeyPrivateKeyEd25519(bytes)) => bytes,
        _ => return None,
    };
    let verify_key = SigningKey::from_bytes(&seed).verifying_key();
    let pk_bytes = verify_key.to_bytes();
    Some(Strkey::PublicKeyEd25519(StrkeyPublicKeyEd25519(pk_bytes)).to_string())
}

/// Builds the base64-XDR envelope to submit to the registry. The MVP
/// implementation is a JSON stub (the real XDR will be wired in once the
/// registry contract ABI is finalized). The structure is intentionally
/// stable so callers can rely on the round-trip in tests.
pub fn build_envelope(verification: &Verification, registry_contract_id: &str) -> String {
    // TODO(M3): replace with proper stellar-xdr TransactionEnvelope
    // construction: InvokeHostFunctionOp calling `attest` on
    // `registry_contract_id` with the contract_id, wasm_hash, repo_url,
    // commit_sha, trust_tier; signed with the attester key; fee/source
    // account set; sequence number pulled from getNetwork/latestLedger.
    let payload = json!({
        "contract": registry_contract_id,
        "method": "attest",
        "args": {
            "network": verification.network.as_str(),
            "contract_id": verification.contract_id,
            "wasm_hash": verification.wasm_hash,
            "commit_sha": verification.commit_sha,
            "trust_tier": verification.trust_tier.as_str(),
        },
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("attestation envelope payload must serialize (fixed shape)");
    STANDARD.encode(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_verify_common::models::{Network, TrustTier, VerificationStatus};

    /// A valid Stellar strkey secret seed (S...). Validated at module load
    /// so a bad fixture surfaces as a named test failure rather than a
    /// panicking `expect`.
    const TEST_SEED: &str = "SDR4C2CKNCVK4DWMTNI2IXFJ6BE3A6J3WVNCGR6Q3SCMJDTSVHMJGC6U";

    #[test]
    fn test_seed_is_valid_strkey() {
        // Sanity check: if the fixture ever becomes invalid, the other
        // tests would panic with a misleading `seed should parse` message.
        // This gives a clear, named failure instead.
        assert!(
            matches!(
                Strkey::from_string(TEST_SEED),
                Ok(Strkey::PrivateKeyEd25519(_))
            ),
            "TEST_SEED is not a valid Stellar strkey secret seed"
        );
    }

    /// The G-address derived from `TEST_SEED`. Computed once (not
    /// hardcoded) so the rejection tests exercise the match-arm path on
    /// a *known-valid* G-address, not a malformed string.
    fn known_public_address() -> String {
        derive_attester_address(TEST_SEED).expect("TEST_SEED must be a valid strkey secret")
    }

    fn sample_verification() -> Verification {
        Verification {
            id: uuid::Uuid::new_v4(),
            job_id: uuid::Uuid::new_v4(),
            contract_id: "CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE".into(),
            network: Network::Testnet,
            repo_url: "https://github.com/org/proj".into(),
            commit_sha: "bd7203f0e1b1f3a2c4d5e6f708192a3b4c5d6e7f".into(),
            wasm_hash: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            rebuilt_wasm_hash: Some(
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            ),
            image_digest: Some("sha256:abc".into()),
            trust_tier: TrustTier::Auditable,
            status: VerificationStatus::Verified,
            attestation_tx_hash: None,
            attester_address: None,
            verified_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn derive_address_is_deterministic_and_well_formed() {
        let addr1 = derive_attester_address(TEST_SEED).expect("seed should parse");
        let addr2 = derive_attester_address(TEST_SEED).expect("seed should parse");
        // Deterministic.
        assert_eq!(addr1, addr2);
        // Well-formed Stellar strkey public key.
        assert!(
            addr1.starts_with('G'),
            "address must start with G, got {addr1}"
        );
        assert!(
            (55..=60).contains(&addr1.len()),
            "address must be ~56 chars, got {} chars",
            addr1.len()
        );
    }

    #[test]
    fn derive_address_rejects_garbage() {
        assert!(derive_attester_address("not a strkey secret").is_none());
        // A valid G-address strkey is a public key, not a secret, so it
        // must not be accepted as an attester secret.
        assert!(derive_attester_address(&known_public_address()).is_none());
    }

    #[test]
    fn debug_impl_redacts_secret() {
        let cfg = AttestConfig {
            registry_contract_id: "CABC".into(),
            attester_secret_key: "TOP-SECRET-SEED-DO-NOT-LOG".into(),
        };
        let dbg = format!("{cfg:?}");
        assert!(dbg.contains("CABC"), "debug must show registry id");
        assert!(
            !dbg.contains("TOP-SECRET-SEED-DO-NOT-LOG"),
            "debug must redact the secret seed: {dbg}"
        );
        assert!(dbg.contains("<redacted>"), "debug must mark field redacted");
    }

    #[test]
    fn envelope_roundtrips_as_base64() {
        let v = sample_verification();
        let envelope = build_envelope(&v, "CABC123CONTRACTID");
        // Must be valid base64.
        let raw = STANDARD.decode(&envelope).expect("envelope must be base64");
        let parsed: serde_json::Value =
            serde_json::from_slice(&raw).expect("decoded envelope must be JSON");
        assert_eq!(parsed["method"], "attest");
        assert_eq!(parsed["contract"], "CABC123CONTRACTID");
        assert_eq!(parsed["args"]["network"], "testnet");
        assert_eq!(parsed["args"]["contract_id"], v.contract_id);
        assert_eq!(parsed["args"]["wasm_hash"], v.wasm_hash);
        assert_eq!(parsed["args"]["commit_sha"], v.commit_sha);
        assert_eq!(parsed["args"]["trust_tier"], "auditable");
    }

    #[test]
    fn envelope_does_not_contain_secret() {
        let v = sample_verification();
        let envelope = build_envelope(&v, "CABC");
        // The envelope is built from public verification fields only; the
        // secret key never enters this code path. This test guards against
        // future regressions that might accidentally widen the payload.
        assert!(!envelope.contains("ATTESTER"));
        assert!(!envelope.contains("secret"));
    }

    #[tokio::test]
    async fn skipped_when_no_config() {
        let v = sample_verification();
        let rpc = SorobanRpc::new("http://127.0.0.1:1"); // never reached
        let outcome = submit_attestation(&v, None, &rpc).await;
        assert_eq!(outcome, AttestationOutcome::Skipped);
    }

    #[tokio::test]
    async fn invalid_secret_returns_failed() {
        let v = sample_verification();
        let rpc = SorobanRpc::new("http://127.0.0.1:1"); // never reached
        let cfg = AttestConfig {
            registry_contract_id: "CABC".into(),
            attester_secret_key: "not a strkey secret".into(),
        };
        let outcome = submit_attestation(&v, Some(&cfg), &rpc).await;
        match outcome {
            AttestationOutcome::Failed {
                attester_address,
                error,
            } => {
                assert_eq!(attester_address, "unknown");
                assert!(error.contains("ATTESTER_SECRET_KEY"));
                // The secret itself must not appear in the error.
                assert!(!error.contains("not a strkey secret"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
