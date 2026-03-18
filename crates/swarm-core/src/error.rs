//! Core error types for the AiOfficeSwarm framework.
//!
//! All errors in the framework are variants of [`SwarmError`]. Downstream crates
//! should use [`SwarmResult`] as their return type and convert lower-level errors
//! via `From` implementations or the `?` operator.
//!
//! ## Design rationale
//! A single, structured error enum (rather than `anyhow::Error` or `Box<dyn Error>`)
//! allows call sites to pattern-match on specific failure modes and make runtime
//! decisions (e.g., retry on `Timeout`, escalate on `PolicyViolation`).

use thiserror::Error;

use crate::identity::{AgentId, PolicyId, TaskId};

/// The primary result type used throughout the framework.
pub type SwarmResult<T> = Result<T, SwarmError>;

/// All structured errors that can occur within the AiOfficeSwarm framework.
#[allow(missing_docs)]
#[derive(Debug, Error)]
pub enum SwarmError {
    // ── Agent errors ───────────────────────────────────────────────────────
    /// An agent with the given ID was not found in the registry.
    #[error("agent not found: {id}")]
    AgentNotFound { id: AgentId },

    /// The agent is in a state that does not allow the requested operation.
    #[error("agent {id} is in an invalid state for this operation: {reason}")]
    AgentInvalidState { id: AgentId, reason: String },

    /// The agent does not possess the required capability.
    #[error("agent {id} lacks capability '{capability}'")]
    AgentMissingCapability { id: AgentId, capability: String },

    /// The agent's resource limits were exceeded.
    #[error("agent {id} exceeded resource limit: {resource}")]
    AgentResourceExceeded { id: AgentId, resource: String },

    // ── Task errors ────────────────────────────────────────────────────────
    /// A task with the given ID was not found.
    #[error("task not found: {id}")]
    TaskNotFound { id: TaskId },

    /// The task specification is invalid or malformed.
    #[error("invalid task specification: {reason}")]
    InvalidTaskSpec { reason: String },

    /// The task timed out before completing.
    #[error("task {id} timed out after {elapsed_ms}ms")]
    TaskTimeout { id: TaskId, elapsed_ms: u64 },

    /// The task failed with an agent-reported error.
    #[error("task {id} failed: {reason}")]
    TaskFailed { id: TaskId, reason: String },

    // ── Policy errors ──────────────────────────────────────────────────────
    /// A policy evaluation denied the requested action.
    #[error("policy {policy_id} denied action '{action}': {reason}")]
    PolicyViolation {
        policy_id: PolicyId,
        action: String,
        reason: String,
    },

    /// The policy with the given ID could not be found.
    #[error("policy not found: {id}")]
    PolicyNotFound { id: PolicyId },

    // ── Authorization errors ───────────────────────────────────────────────
    /// The subject does not have the required permission.
    #[error("permission denied: subject '{subject}' lacks '{permission}' on '{resource}'")]
    PermissionDenied {
        subject: String,
        permission: String,
        resource: String,
    },

    // ── Plugin errors ──────────────────────────────────────────────────────
    /// A plugin failed to initialize.
    #[error("plugin '{name}' failed to initialize: {reason}")]
    PluginInitFailed { name: String, reason: String },

    /// A plugin operation returned an error.
    #[error("plugin '{name}' operation failed: {reason}")]
    PluginOperationFailed { name: String, reason: String },

    /// A plugin version is incompatible with the host framework version.
    #[error("plugin '{name}' version {plugin_version} is incompatible with host {host_version}")]
    PluginVersionMismatch {
        name: String,
        plugin_version: String,
        host_version: String,
    },

    // ── Configuration errors ───────────────────────────────────────────────
    /// A required configuration value is missing.
    #[error("missing configuration key: '{key}'")]
    ConfigMissing { key: String },

    /// A configuration value failed validation.
    #[error("invalid configuration value for '{key}': {reason}")]
    ConfigInvalid { key: String, reason: String },

    // ── Runtime / infrastructure errors ───────────────────────────────────
    /// A channel send/receive operation failed (typically means a component
    /// has shut down unexpectedly).
    #[error("channel communication failed: {reason}")]
    ChannelError { reason: String },

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error occurred.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// An unexpected internal error occurred. This is a catch-all for bugs.
    #[error("internal error: {reason}")]
    Internal { reason: String },
}

impl SwarmError {
    /// Returns `true` if this error represents a transient failure that *may*
    /// succeed on retry (e.g., timeout, channel errors).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SwarmError::TaskTimeout { .. } | SwarmError::ChannelError { .. }
        )
    }

    /// Returns `true` if this error is a hard security/policy violation that
    /// should never be retried.
    pub fn is_security_error(&self) -> bool {
        matches!(
            self,
            SwarmError::PolicyViolation { .. } | SwarmError::PermissionDenied { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{AgentId, TaskId};

    #[test]
    fn policy_violation_is_security_error() {
        let err = SwarmError::PolicyViolation {
            policy_id: PolicyId::new(),
            action: "create_agent".into(),
            reason: "quota exceeded".into(),
        };
        assert!(err.is_security_error());
        assert!(!err.is_retryable());
    }

    #[test]
    fn task_timeout_is_retryable() {
        let err = SwarmError::TaskTimeout {
            id: TaskId::new(),
            elapsed_ms: 5000,
        };
        assert!(err.is_retryable());
        assert!(!err.is_security_error());
    }

    #[test]
    fn agent_not_found_display() {
        let id = AgentId::new();
        let err = SwarmError::AgentNotFound { id };
        assert!(err.to_string().contains("agent not found"));
    }
}
