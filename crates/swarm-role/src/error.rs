//! Role-specific error types.

use thiserror::Error;

/// Errors specific to the role subsystem.
#[derive(Debug, Error)]
pub enum RoleError {
    /// A role file could not be read from disk.
    #[error("failed to read role file {path}: {source}")]
    FileRead {
        /// Path that was attempted.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A role file could not be parsed.
    #[error("failed to parse role file {path}: {reason}")]
    ParseFailed {
        /// Path of the file.
        path: String,
        /// Description of what went wrong.
        reason: String,
    },

    /// Normalization of a raw role source failed.
    #[error("normalization failed for role '{name}': {reason}")]
    NormalizationFailed {
        /// Role name (if available).
        name: String,
        /// Description of what went wrong.
        reason: String,
    },

    /// Validation of a role specification failed.
    #[error("validation failed for role '{role_id}': {issues:?}")]
    ValidationFailed {
        /// The role ID or name.
        role_id: String,
        /// The list of validation issues.
        issues: Vec<String>,
    },

    /// A role was not found in the registry.
    #[error("role not found: {id}")]
    NotFound {
        /// The missing role identifier.
        id: String,
    },

    /// A duplicate role ID was detected.
    #[error("duplicate role: {id}")]
    Duplicate {
        /// The conflicting role identifier.
        id: String,
    },

    /// A relationship referenced a role that does not exist.
    #[error("unknown role referenced in hierarchy: {name}")]
    UnknownRelationship {
        /// The referenced role name.
        name: String,
    },

    /// A conflict was detected during resolution.
    #[error("role conflict for '{role_id}': {reason}")]
    ConflictDetected {
        /// The role ID.
        role_id: String,
        /// Description of the conflict.
        reason: String,
    },

    /// TOML deserialization error for structured role files.
    #[error("TOML parse error: {0}")]
    TomlError(#[from] toml::de::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Convenience result type for role operations.
pub type RoleResult<T> = std::result::Result<T, RoleError>;
