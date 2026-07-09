use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::models::Network;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub api_bind_addr: SocketAddr,
    pub rpc_url_testnet: String,
    pub rpc_url_mainnet: Option<String>,
    pub worker_poll_interval_ms: u64,
    pub max_concurrent_builds: usize,
    pub build_timeout_secs: u64,
    pub build_scratch_dir: PathBuf,
    pub default_build_image: String,
    pub trusted_image_digests: Vec<String>,
    pub auditable_image_registries: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            api_bind_addr: parsed("API_BIND_ADDR", "0.0.0.0:8080")?,
            rpc_url_testnet: or_default("RPC_URL_TESTNET", "https://soroban-testnet.stellar.org"),
            rpc_url_mainnet: optional("RPC_URL_MAINNET"),
            worker_poll_interval_ms: parsed("WORKER_POLL_INTERVAL_MS", "1000")?,
            max_concurrent_builds: parsed("MAX_CONCURRENT_BUILDS", "2")?,
            build_timeout_secs: parsed("BUILD_TIMEOUT_SECS", "1800")?,
            build_scratch_dir: PathBuf::from(or_default("BUILD_SCRATCH_DIR", "./builds")),
            default_build_image: or_default(
                "DEFAULT_BUILD_IMAGE",
                "docker.io/stellar/stellar-contract-build:latest",
            ),
            trusted_image_digests: list("TRUSTED_IMAGE_DIGESTS"),
            auditable_image_registries: list("AUDITABLE_IMAGE_REGISTRIES"),
        })
    }

    pub fn rpc_url(&self, network: Network) -> Result<&str> {
        match network {
            Network::Testnet => Ok(&self.rpc_url_testnet),
            Network::Mainnet => self
                .rpc_url_mainnet
                .as_deref()
                .ok_or_else(|| Error::Config("RPC_URL_MAINNET is not set".into())),
        }
    }
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn required(name: &str) -> Result<String> {
    optional(name).ok_or_else(|| Error::Config(format!("{name} must be set")))
}

fn or_default(name: &str, default: &str) -> String {
    optional(name).unwrap_or_else(|| default.to_string())
}

fn parsed<T>(name: &str, default: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    or_default(name, default)
        .parse()
        .map_err(|e| Error::Config(format!("invalid {name}: {e}")))
}

fn list(name: &str) -> Vec<String> {
    optional(name)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
