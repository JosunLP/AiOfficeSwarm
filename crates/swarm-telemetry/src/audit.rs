//! Audit logger: records security-sensitive operations with structured entries.
//!
//! The audit log is an append-only record of actions that require
//! accountability (agent registration, task scheduling, policy evaluations,
//! plugin loads, etc.). Audit entries are always stored in the in-memory audit
//! log even if the accompanying tracing event is filtered by log level.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// The action that was performed (e.g., `"agent.register"`, `"task.submit"`).
    pub action: String,
    /// The subject that performed the action.
    pub subject: String,
    /// The resource that was acted upon.
    pub resource: String,
    /// Whether the action was permitted or denied.
    pub outcome: AuditOutcome,
    /// Additional context (free-form JSON).
    pub context: serde_json::Value,
}

/// Whether an audited action was allowed or denied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    /// The action was permitted and executed.
    Allowed,
    /// The action was denied (e.g., by a policy).
    Denied {
        /// Reason for denial.
        reason: String,
    },
}

/// An in-memory, thread-safe audit logger.
///
/// In production deployments this should be backed by a durable store.
/// The current implementation is suitable for development, testing, and
/// single-node deployments where log persistence is handled externally
/// (e.g., by writing tracing events to a log aggregator).
#[derive(Clone, Default)]
pub struct AuditLogger {
    entries: Arc<Mutex<Vec<AuditEntry>>>,
}

impl AuditLogger {
    /// Create a new empty audit logger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an audit entry.
    ///
    /// If the internal mutex is poisoned (a previous writer panicked), the
    /// lock is recovered so that auditing can continue rather than causing a
    /// cascading panic.
    pub fn record(&self, entry: AuditEntry) {
        tracing::info!(
            action = %entry.action,
            subject = %entry.subject,
            resource = %entry.resource,
            outcome = ?entry.outcome,
            "[AUDIT]"
        );
        match self.entries.lock() {
            Ok(mut entries) => entries.push(entry),
            Err(poisoned) => poisoned.into_inner().push(entry),
        }
    }

    /// Convenience method to log an allowed action.
    pub fn allowed(
        &self,
        action: impl Into<String>,
        subject: impl Into<String>,
        resource: impl Into<String>,
    ) {
        self.record(AuditEntry {
            timestamp: Utc::now(),
            action: action.into(),
            subject: subject.into(),
            resource: resource.into(),
            outcome: AuditOutcome::Allowed,
            context: serde_json::Value::Null,
        });
    }

    /// Convenience method to log a denied action.
    pub fn denied(
        &self,
        action: impl Into<String>,
        subject: impl Into<String>,
        resource: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.record(AuditEntry {
            timestamp: Utc::now(),
            action: action.into(),
            subject: subject.into(),
            resource: resource.into(),
            outcome: AuditOutcome::Denied {
                reason: reason.into(),
            },
            context: serde_json::Value::Null,
        });
    }

    /// Return all recorded entries (for testing or reporting).
    pub fn entries(&self) -> Vec<AuditEntry> {
        match self.entries.lock() {
            Ok(entries) => entries.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Clear all entries.
    pub fn clear(&self) {
        match self.entries.lock() {
            Ok(mut entries) => entries.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_retrieve() {
        let logger = AuditLogger::new();
        logger.allowed("task.submit", "user:alice", "task-queue");
        logger.denied(
            "agent.delete",
            "user:bob",
            "agent:xyz",
            "insufficient permissions",
        );

        let entries = logger.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].outcome, AuditOutcome::Allowed);
        assert!(matches!(&entries[1].outcome, AuditOutcome::Denied { .. }));
    }
}
