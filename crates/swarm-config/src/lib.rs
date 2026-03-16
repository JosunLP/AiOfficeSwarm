//! # swarm-config
//!
//! Configuration management for the AiOfficeSwarm framework.
//!
//! This crate provides:
//!
//! - [`SwarmConfig`]: The top-level configuration structure for the framework.
//! - [`ConfigLoader`]: Loads and merges configuration from TOML files and
//!   environment variables.
//! - [`SecretsProvider`] trait: An abstraction for secret retrieval that
//!   decouples the framework from specific secret management backends
//!   (Vault, AWS Secrets Manager, env vars, etc.).
//!
//! ## Configuration precedence (highest to lowest)
//! 1. Environment variables (`SWARM_*` prefix)
//! 2. Configuration file (TOML)
//! 3. Compiled-in defaults

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod loader;
pub mod model;
pub mod secrets;

pub use loader::ConfigLoader;
pub use model::SwarmConfig;
pub use secrets::{EnvSecretsProvider, SecretsProvider};
