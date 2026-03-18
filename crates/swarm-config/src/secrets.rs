//! Secrets abstraction: decouples the framework from specific secret backends.
//!
//! The [`SecretsProvider`] trait allows the framework to retrieve secrets
//! (API keys, tokens, passwords) without being coupled to a specific backend
//! such as HashiCorp Vault, AWS Secrets Manager, or plain environment variables.
//!
//! ## Built-in providers
//! - [`EnvSecretsProvider`]: Reads secrets from environment variables.
//!   Suitable for development and simple deployments.
//!
//! ## Security note
//! Secrets are returned as `String`. Callers are responsible for clearing
//! secrets from memory when no longer needed. Future versions may use a
//! `SecretString` type with automatic zeroing on drop.

use swarm_core::error::{SwarmError, SwarmResult};

/// Trait for retrieving secrets by name.
///
/// Implement this trait to integrate with enterprise secret stores.
pub trait SecretsProvider: Send + Sync {
    /// Retrieve a secret by its logical name.
    ///
    /// The `name` is an application-level key (e.g., `"github.api_token"`).
    /// The implementation maps this to the appropriate backend key.
    fn get_secret(&self, name: &str) -> SwarmResult<String>;
}

/// A [`SecretsProvider`] that reads secrets from environment variables.
///
/// The environment variable name is constructed by converting the secret name
/// to `SCREAMING_SNAKE_CASE` and optionally prepending a prefix.
///
/// ## Example
/// A secret named `"github.api_token"` with prefix `"SWARM"` is looked up
/// as the environment variable `SWARM_GITHUB_API_TOKEN`.
pub struct EnvSecretsProvider {
    /// Optional prefix added to all variable lookups.
    prefix: Option<String>,
}

impl EnvSecretsProvider {
    /// Create a provider with no prefix.
    pub fn new() -> Self {
        Self { prefix: None }
    }

    /// Create a provider with the given prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
        }
    }

    fn env_var_name(&self, name: &str) -> String {
        let normalized = name
            .to_uppercase()
            .replace('.', "_")
            .replace('-', "_");
        match &self.prefix {
            Some(p) => format!("{}_{}", p.to_uppercase(), normalized),
            None => normalized,
        }
    }
}

impl Default for EnvSecretsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretsProvider for EnvSecretsProvider {
    fn get_secret(&self, name: &str) -> SwarmResult<String> {
        let var = self.env_var_name(name);
        std::env::var(&var).map_err(|_| SwarmError::ConfigMissing { key: var })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_name_without_prefix() {
        let p = EnvSecretsProvider::new();
        assert_eq!(p.env_var_name("github.api_token"), "GITHUB_API_TOKEN");
    }

    #[test]
    fn env_var_name_with_prefix() {
        let p = EnvSecretsProvider::with_prefix("SWARM");
        assert_eq!(p.env_var_name("github.api_token"), "SWARM_GITHUB_API_TOKEN");
    }

    #[test]
    fn missing_env_var_returns_error() {
        let p = EnvSecretsProvider::with_prefix("SWARM_TEST_NONEXISTENT_XYZ");
        assert!(p.get_secret("some.secret").is_err());
    }
}
