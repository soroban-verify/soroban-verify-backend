//! Input validation shared by the API (request intake) and the worker
//! (defense in depth before shelling out to git/docker).

use crate::error::{Error, Result};

/// Soroban contract IDs are 56-char C-prefixed strkeys (base32: A-Z, 2-7).
///
/// Validates the full CRC-16 checksum via `stellar-strkey`. A fast-path
/// pre-filter (length + char set) rejects obviously invalid inputs before
/// the checksum check.
pub fn contract_id(s: &str) -> Result<()> {
    // Fast path: length + character set pre-filter.
    if s.len() != 56
        || !s.starts_with('C')
        || !s
            .bytes()
            .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b))
    {
        return Err(Error::InvalidInput(format!(
            "invalid contract id: {s} (expected 56-char C… strkey)"
        )));
    }

    // Full CRC-16 checksum validation via stellar-strkey.
    stellar_strkey::Contract::from_string(s).map_err(|e| {
        Error::InvalidInput(format!(
            "invalid contract id: {s} (checksum validation failed: {e})"
        ))
    })?;

    Ok(())
}

/// Full 40-char hex commit SHA. Pinning to an exact commit (not a branch or
/// tag) is required for replayability.
pub fn commit_sha(s: &str) -> Result<()> {
    if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "invalid commit sha: {s} (expected full 40-char hex sha)"
        )))
    }
}

/// Only https git remotes are accepted. Also rejects anything that could be
/// interpreted as a git CLI flag.
pub fn repo_url(s: &str) -> Result<()> {
    if s.starts_with("https://") && !s.contains(char::is_whitespace) {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "invalid repo url: {s} (only https:// remotes are supported)"
        )))
    }
}

/// Hex-encoded sha256 (64 chars).
pub fn wasm_hash(s: &str) -> Result<()> {
    if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "invalid wasm hash: {s} (expected 64-char hex sha256)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_contract_id() {
        assert!(contract_id("CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE").is_ok());
    }

    #[test]
    fn rejects_bad_contract_ids() {
        assert!(contract_id("GA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE").is_err());
        assert!(contract_id("CA3D5KRYM6CB7OWQ").is_err());
        assert!(contract_id("ca3d5krym6cb7owq6twyrr3z4t7gnzlkerynzgga5soaopify6yqgaxe").is_err());
    }

    #[test]
    fn rejects_checksum_invalid_contract_id() {
        // A valid 56-char C-prefixed strkey with one character flipped to
        // break the CRC-16 checksum.
        assert!(contract_id("CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXF").is_err());
    }

    #[test]
    fn validates_commit_shas() {
        assert!(commit_sha("bd7203f0e1b1f3a2c4d5e6f708192a3b4c5d6e7f").is_ok());
        assert!(commit_sha("main").is_err());
        assert!(commit_sha("bd7203f").is_err());
    }

    #[test]
    fn validates_repo_urls() {
        assert!(repo_url("https://github.com/org/project").is_ok());
        assert!(repo_url("git@github.com:org/project.git").is_err());
        assert!(repo_url("--upload-pack=evil").is_err());
    }
}
