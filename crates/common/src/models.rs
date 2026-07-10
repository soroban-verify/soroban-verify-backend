use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::FromRow;
use uuid::Uuid;

/// Implements `sqlx::Type`/`Encode`/`Decode` for an enum stored as a Postgres
/// TEXT column, delegating to the enum's `as_str`/`FromStr` impls.
macro_rules! pg_text_enum {
    ($ty:ty) => {
        impl sqlx::Type<sqlx::Postgres> for $ty {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <&str as sqlx::Type<sqlx::Postgres>>::type_info()
            }
            fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                <&str as sqlx::Type<sqlx::Postgres>>::compatible(ty)
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::Postgres> for $ty {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> std::result::Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <&str as sqlx::Encode<'q, sqlx::Postgres>>::encode_by_ref(&self.as_str(), buf)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $ty {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
                let s = <&str as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
                Ok(s.parse::<Self>()?)
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
}

impl Network {
    pub fn as_str(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
        }
    }
}

impl std::str::FromStr for Network {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            other => Err(format!("unknown network: {other}")),
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pg_text_enum!(Network);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    /// Rebuilt Wasm matched the on-chain hash.
    Verified,
    /// Build succeeded but the rebuilt Wasm did not match the on-chain hash.
    Mismatch,
    /// The verification could not be completed (clone/build/RPC failure).
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Verified => "verified",
            JobStatus::Mismatch => "mismatch",
            JobStatus::Failed => "failed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Verified | JobStatus::Mismatch | JobStatus::Failed
        )
    }
}

impl std::str::FromStr for JobStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "queued" => Ok(JobStatus::Queued),
            "running" => Ok(JobStatus::Running),
            "verified" => Ok(JobStatus::Verified),
            "mismatch" => Ok(JobStatus::Mismatch),
            "failed" => Ok(JobStatus::Failed),
            other => Err(format!("unknown job status: {other}")),
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pg_text_enum!(JobStatus);

/// The multi-dimensional trust level of a reproduced build (see README:
/// reproducibility alone is not faithfulness to source — the tier reflects
/// how much the *build image* itself can be trusted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    /// Reproduced inside an SDF-allowlisted trusted image.
    Trusted,
    /// Reproduced inside a publicly auditable, pinned image.
    Auditable,
    /// Reproduced inside an arbitrary, deployer-supplied image.
    DeployerSupplied,
}

impl TrustTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrustTier::Trusted => "trusted",
            TrustTier::Auditable => "auditable",
            TrustTier::DeployerSupplied => "deployer_supplied",
        }
    }
}

impl std::str::FromStr for TrustTier {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "trusted" => Ok(TrustTier::Trusted),
            "auditable" => Ok(TrustTier::Auditable),
            "deployer_supplied" => Ok(TrustTier::DeployerSupplied),
            other => Err(format!("unknown trust tier: {other}")),
        }
    }
}

impl std::fmt::Display for TrustTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pg_text_enum!(TrustTier);

/// Outcome recorded in the canonical `verifications` table. Failed builds stay
/// on the job; only completed byte-comparisons are published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Mismatch,
}

impl VerificationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerificationStatus::Verified => "verified",
            VerificationStatus::Mismatch => "mismatch",
        }
    }
}

impl std::str::FromStr for VerificationStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "verified" => Ok(VerificationStatus::Verified),
            "mismatch" => Ok(VerificationStatus::Mismatch),
            other => Err(format!("unknown verification status: {other}")),
        }
    }
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pg_text_enum!(VerificationStatus);

/// Submitter-supplied build parameters (SEP-58 metadata, when present in the
/// contract Wasm, can pre-fill these — see `crate::sep58`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// Cargo package to build when the repo is a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Build image reference. Should be digest-pinned; determines the trust
    /// tier of the result. Defaults to the service's configured trusted image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Cargo features to enable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Dev/test escape hatch: expected on-chain Wasm hash (hex). Used until
    /// on-chain hash resolution via getLedgerEntries lands (see
    /// `rpc::SorobanRpc::contract_wasm_hash`). Ignored in production mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_wasm_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct VerificationJob {
    pub id: Uuid,
    pub contract_id: String,
    pub network: Network,
    pub repo_url: String,
    pub commit_sha: String,
    pub build_config: Json<BuildConfig>,
    pub status: JobStatus,
    pub trust_tier: Option<TrustTier>,
    pub error: Option<String>,
    pub attempts: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub contract_id: String,
    pub network: Network,
    pub repo_url: String,
    pub commit_sha: String,
    pub build_config: BuildConfig,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Verification {
    pub id: Uuid,
    pub job_id: Uuid,
    pub contract_id: String,
    pub network: Network,
    pub repo_url: String,
    pub commit_sha: String,
    /// On-chain Wasm hash (hex-encoded sha256).
    pub wasm_hash: String,
    /// Hash of the Wasm we rebuilt from source.
    pub rebuilt_wasm_hash: Option<String>,
    pub image_digest: Option<String>,
    pub trust_tier: TrustTier,
    pub status: VerificationStatus,
    /// Hash of the on-chain `attest` transaction submitted to the verification
    /// registry contract (M3). NULL until submission succeeds (or if M3 is
    /// disabled via missing `REGISTRY_CONTRACT_ID` / `ATTESTER_SECRET_KEY`).
    pub attestation_tx_hash: Option<String>,
    /// Stellar strkey G-address of the attester that produced the on-chain
    /// attestation, derived from `ATTESTER_SECRET_KEY`.
    pub attester_address: Option<String>,
    /// Result of the SEP-58 metadata cross-check against the on-chain Wasm
    /// (issue #2). `None` when the cross-check could not be performed
    /// (on-chain bytes unavailable, or no `contractmetav0` section).
    /// `Some(true)` = mismatch between embedded and submitted
    /// `source_repo`/`commit_sha`. `Some(false)` = values agreed.
    pub sep58_mismatch: Option<bool>,
    pub verified_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewVerification {
    pub job_id: Uuid,
    pub contract_id: String,
    pub network: Network,
    pub repo_url: String,
    pub commit_sha: String,
    pub wasm_hash: String,
    pub rebuilt_wasm_hash: Option<String>,
    pub image_digest: Option<String>,
    pub trust_tier: TrustTier,
    pub status: VerificationStatus,
    /// Initially `None` for the first `upsert_verification`; populated by a
    /// follow-up `update_attestation` call after a successful M3 submission.
    pub attestation_tx_hash: Option<String>,
    pub attester_address: Option<String>,
    /// SEP-58 cross-check result, threaded through from the pipeline. `None`
    /// means the cross-check could not be performed (see `Verification`).
    pub sep58_mismatch: Option<bool>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct BuildLogLine {
    pub id: i64,
    pub job_id: Uuid,
    pub seq: i32,
    pub line: String,
    pub created_at: DateTime<Utc>,
}
