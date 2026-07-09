//! Canonical byte comparison. On-chain, a contract's code is addressed by the
//! sha256 of its Wasm, so comparing hashes IS comparing bytes.

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn hashes_match(rebuilt_hex: &str, onchain_hex: &str) -> bool {
    rebuilt_hex.eq_ignore_ascii_case(onchain_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_empty_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn comparison_is_case_insensitive() {
        assert!(hashes_match("ABC123", "abc123"));
        assert!(!hashes_match("abc123", "abc124"));
    }
}
