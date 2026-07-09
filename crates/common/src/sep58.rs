//! SEP-58 reproducible-build metadata resolution.
//!
//! SEP-58 (draft) defines the metadata vocabulary a Soroban contract embeds —
//! or surfaces off-chain — so a verifier holding the source can rebuild the
//! Wasm and confirm the bytes match: source repo, commit, toolchain version,
//! and build flags.
//!
//! Spec-traceability: every field here maps to a SEP-58 attribute; if the
//! draft changes, this module tracks it within one release cycle (see
//! "Maintenance Commitment" in the top-level README).

use serde::{Deserialize, Serialize};

/// Build provenance metadata recovered from a contract's Wasm (custom
/// sections) or from an off-chain source per SEP-58.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sep58Metadata {
    pub source_repo: Option<String>,
    pub commit_sha: Option<String>,
    pub rust_version: Option<String>,
    pub soroban_cli_version: Option<String>,
    pub build_image: Option<String>,
}

/// Extracts SEP-58 metadata embedded in the contract Wasm.
///
/// TODO(M2): parse Wasm custom sections (`contractmetav0` today; the SEP-58
/// section name once the draft stabilizes) using the `wasmparser` crate, and
/// cross-check the submitted repo/commit against the embedded values —
/// mismatches should be surfaced on the verification record.
pub fn resolve_from_wasm(_wasm: &[u8]) -> Option<Sep58Metadata> {
    None
}
