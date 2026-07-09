//! Image trust classification.
//!
//! Reproducibility alone is not faithfulness to source: a hostile build image
//! can deterministically rewrite bytes and still pass byte-comparison. The
//! trust tier therefore reflects how much the *image* can be trusted, not
//! just whether the build reproduced.

use std::collections::HashSet;

use crate::config::Config;
use crate::models::TrustTier;

#[derive(Debug, Clone)]
pub struct TrustPolicy {
    /// sha256 digests of SDF-allowlisted trusted images.
    trusted_digests: HashSet<String>,
    /// Registry/namespace prefixes considered publicly auditable.
    auditable_registries: Vec<String>,
}

impl TrustPolicy {
    pub fn new(trusted_digests: Vec<String>, auditable_registries: Vec<String>) -> Self {
        Self {
            trusted_digests: trusted_digests.into_iter().collect(),
            auditable_registries,
        }
    }

    pub fn from_config(cfg: &Config) -> Self {
        Self::new(
            cfg.trusted_image_digests.clone(),
            cfg.auditable_image_registries.clone(),
        )
    }

    /// Classifies an image reference into a trust tier.
    ///
    /// TODO(M4): resolve tags to digests before classification (a tag can be
    /// re-pointed after verification; only digest-pinned references should
    /// ever reach the `trusted` tier) and verify image signatures/provenance.
    pub fn classify(&self, image: &str) -> TrustTier {
        if let Some(digest) = image.split("@sha256:").nth(1) {
            if self.trusted_digests.contains(digest)
                || self.trusted_digests.contains(&format!("sha256:{digest}"))
            {
                return TrustTier::Trusted;
            }
        }
        if self
            .auditable_registries
            .iter()
            .any(|prefix| image.starts_with(prefix.as_str()))
        {
            return TrustTier::Auditable;
        }
        TrustTier::DeployerSupplied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> TrustPolicy {
        TrustPolicy::new(vec!["abc123".into()], vec!["docker.io/stellar/".into()])
    }

    #[test]
    fn digest_pinned_allowlisted_image_is_trusted() {
        assert_eq!(
            policy().classify("docker.io/stellar/stellar-contract-build@sha256:abc123"),
            TrustTier::Trusted
        );
    }

    #[test]
    fn known_registry_without_trusted_digest_is_auditable() {
        assert_eq!(
            policy().classify("docker.io/stellar/stellar-contract-build:v1.2.3"),
            TrustTier::Auditable
        );
    }

    #[test]
    fn unknown_image_is_deployer_supplied() {
        assert_eq!(
            policy().classify("ghcr.io/someone/custom-builder:latest"),
            TrustTier::DeployerSupplied
        );
    }
}
