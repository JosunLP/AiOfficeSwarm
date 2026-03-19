//! Retention policy types for memory governance.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::entry::MemoryScope;

/// A retention policy controlling how long memory entries are kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// The scope this policy applies to. If `None`, applies to all scopes.
    pub scope: Option<MemoryScope>,
    /// Maximum age for entries in this scope. Older entries are eligible for removal.
    pub max_age: Option<Duration>,
    /// Maximum number of entries in this scope. Oldest entries are removed first.
    pub max_entries: Option<u64>,
    /// Whether to auto-summarize entries before removal.
    pub auto_summarize: bool,
    /// Summarize entries older than this duration (before removal).
    pub summarize_after: Option<Duration>,
    /// Whether audit logging is required when entries are deleted.
    pub require_audit_on_delete: bool,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            scope: None,
            max_age: None,
            max_entries: None,
            auto_summarize: false,
            summarize_after: None,
            require_audit_on_delete: true,
        }
    }
}

impl RetentionPolicy {
    /// Create a policy that retains entries for the given duration.
    pub fn max_age(duration: Duration) -> Self {
        Self {
            max_age: Some(duration),
            ..Default::default()
        }
    }

    /// Create a policy that retains at most N entries.
    pub fn max_entries(count: u64) -> Self {
        Self {
            max_entries: Some(count),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_policy_defaults() {
        let policy = RetentionPolicy::default();
        assert!(policy.max_age.is_none());
        assert!(policy.max_entries.is_none());
        assert!(policy.require_audit_on_delete);
    }
}
