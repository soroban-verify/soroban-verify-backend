//! Shared types and infrastructure for the soroban-verify backend:
//! configuration, database access, domain models, the Soroban RPC client,
//! SEP-58 metadata resolution, and the image trust policy.

pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod repo;
pub mod rpc;
pub mod sep58;
pub mod trust;
pub mod validate;

pub use error::{Error, Result};
