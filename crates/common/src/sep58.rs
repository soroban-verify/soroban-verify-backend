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
use wasmparser::Payload;

/// The current Soroban convention for the embedded metadata custom section
/// name. The SEP-58 draft section name will replace this constant once
/// finalized.
pub const EMBEDDED_SECTION_NAME: &str = "contractmetav0";

/// Build provenance metadata recovered from a contract's Wasm (custom
/// sections) or from an off-chain source per SEP-58.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sep58Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soroban_cli_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_image: Option<String>,
}

/// Extracts SEP-58 metadata embedded in the contract Wasm's
/// `contractmetav0` custom section. Returns `None` if the section is absent
/// or the bytes cannot be parsed (treated as unknown provenance, not an
/// error — see SEP-58 §4 guidance for verifiers).
///
/// Missing/invalid metadata does **not** fail verification: a contract may
/// simply have been built before the convention was adopted. Callers should
/// record a `None` result on the verification record and proceed.
pub fn resolve_from_wasm(wasm: &[u8]) -> Option<Sep58Metadata> {
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let Ok(payload) = payload else { continue };
        if let Payload::CustomSection(reader) = payload {
            if reader.name() == EMBEDDED_SECTION_NAME {
                return parse_section(reader.data());
            }
        }
    }
    None
}

fn parse_section(data: &[u8]) -> Option<Sep58Metadata> {
    serde_json::from_slice(data).ok()
}

/// Cross-checks the submitter-supplied `repo_url` / `commit_sha` against the
/// values embedded in the contract Wasm. Returns:
///
/// - `None` if either side is missing (no SEP-58 metadata, or no submission
///   values) — i.e. "could not determine".
/// - `Some(true)` if both sides are present and at least one disagrees —
///   a real mismatch that should be surfaced as a warning and a flag on
///   the verification record.
/// - `Some(false)` if both sides are present and agree.
pub fn cross_check(
    embedded: Option<&Sep58Metadata>,
    submitted_repo_url: &str,
    submitted_commit_sha: &str,
) -> Option<bool> {
    let embedded = embedded?;
    let embedded_repo = embedded.source_repo.as_deref()?;
    let embedded_sha = embedded.commit_sha.as_deref()?;
    let repo_match =
        embedded_repo.trim_end_matches('/') == submitted_repo_url.trim_end_matches('/');
    let sha_match = embedded_sha.eq_ignore_ascii_case(submitted_commit_sha);
    Some(!(repo_match && sha_match))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid Wasm module containing a single `contractmetav0`
    /// custom section with the given JSON payload.
    fn wasm_with_section(json_payload: &str) -> Vec<u8> {
        let mut out = Vec::new();
        // Wasm magic + version
        out.extend_from_slice(&[0x00, b'a', b's', b'm']);
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        // Custom section: id 0
        out.push(0x00);
        let name = EMBEDDED_SECTION_NAME.as_bytes();
        let payload = json_payload.as_bytes();
        let mut section = Vec::new();
        section.push(name.len() as u8);
        section.extend_from_slice(name);
        section.extend_from_slice(payload);
        // Section size (LEB128)
        leb128(&mut out, section.len() as u64);
        out.extend_from_slice(&section);
        out
    }

    fn leb128(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    #[test]
    fn parses_valid_metadata_section() {
        let json = r#"{"source_repo":"https://github.com/org/proj","commit_sha":"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef","rust_version":"1.79.0","build_image":"docker.io/stellar/stellar-contract-build@sha256:abc"}"#;
        let wasm = wasm_with_section(json);
        let meta = resolve_from_wasm(&wasm).expect("metadata should parse");
        assert_eq!(
            meta.source_repo.as_deref(),
            Some("https://github.com/org/proj")
        );
        assert_eq!(
            meta.commit_sha.as_deref(),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
        );
        assert_eq!(meta.rust_version.as_deref(), Some("1.79.0"));
        assert_eq!(
            meta.build_image.as_deref(),
            Some("docker.io/stellar/stellar-contract-build@sha256:abc")
        );
    }

    #[test]
    fn returns_none_when_section_missing() {
        // Empty-ish Wasm: magic + version, no custom sections.
        let wasm = vec![0x00, b'a', b's', b'm', 0x01, 0x00, 0x00, 0x00];
        assert!(resolve_from_wasm(&wasm).is_none());
    }

    #[test]
    fn returns_none_on_malformed_json() {
        let wasm = wasm_with_section("this is not json");
        assert!(resolve_from_wasm(&wasm).is_none());
    }

    #[test]
    fn returns_none_on_garbage_bytes() {
        assert!(resolve_from_wasm(b"definitely not wasm").is_none());
    }

    #[test]
    fn cross_check_no_embedded_metadata_is_unknown() {
        let meta = None;
        assert!(cross_check(meta, "https://github.com/org/proj", "abc").is_none());
    }

    #[test]
    fn cross_check_matching_values_passes() {
        let meta = Sep58Metadata {
            source_repo: Some("https://github.com/org/proj".into()),
            commit_sha: Some("ABC123abc123".into()),
            ..Default::default()
        };
        assert_eq!(
            cross_check(Some(&meta), "https://github.com/org/proj", "abc123ABC123"),
            Some(false)
        );
    }

    #[test]
    fn cross_check_repo_mismatch_is_flagged() {
        let meta = Sep58Metadata {
            source_repo: Some("https://github.com/other/proj".into()),
            commit_sha: Some("abc123abc123".into()),
            ..Default::default()
        };
        assert_eq!(
            cross_check(Some(&meta), "https://github.com/org/proj", "abc123abc123"),
            Some(true)
        );
    }

    #[test]
    fn cross_check_commit_mismatch_is_flagged() {
        let meta = Sep58Metadata {
            source_repo: Some("https://github.com/org/proj".into()),
            commit_sha: Some("0000000000000000000000000000000000000000".into()),
            ..Default::default()
        };
        assert_eq!(
            cross_check(Some(&meta), "https://github.com/org/proj", "abc123abc123"),
            Some(true)
        );
    }

    #[test]
    fn cross_check_trailing_slash_on_repo_is_normalized() {
        let meta = Sep58Metadata {
            source_repo: Some("https://github.com/org/proj/".into()),
            commit_sha: Some("abc123abc123".into()),
            ..Default::default()
        };
        assert_eq!(
            cross_check(Some(&meta), "https://github.com/org/proj", "abc123abc123"),
            Some(false)
        );
    }
}
