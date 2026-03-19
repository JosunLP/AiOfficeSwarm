//! Memory access profile and scope rules.
//!
//! An [`MemoryAccessProfile`] defines which memory scopes an agent is allowed
//! to read from and write to.

use serde::{Deserialize, Serialize};

use crate::entry::{MemoryScope, SensitivityLevel};

/// A rule granting access to a specific memory scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAccessRule {
    /// The scope pattern this rule applies to.
    pub scope: MemoryScope,
    /// Whether read access is granted.
    pub read: bool,
    /// Whether write access is granted.
    pub write: bool,
    /// Maximum sensitivity level accessible under this rule.
    pub max_sensitivity: SensitivityLevel,
}

/// Defines which memory scopes and operations an agent may perform.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryAccessProfile {
    /// The access rules. The agent may only access scopes covered by these rules.
    pub rules: Vec<MemoryAccessRule>,
}

impl MemoryAccessProfile {
    /// Create an empty profile (no memory access).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an access rule.
    pub fn add_rule(&mut self, rule: MemoryAccessRule) {
        self.rules.push(rule);
    }

    /// Check whether this profile allows read access to the given scope.
    pub fn can_read(&self, scope: &MemoryScope) -> bool {
        self.rules.iter().any(|r| r.scope == *scope && r.read)
    }

    /// Check whether this profile allows write access to the given scope.
    pub fn can_write(&self, scope: &MemoryScope) -> bool {
        self.rules.iter().any(|r| r.scope == *scope && r.write)
    }

    /// Get the maximum sensitivity level this profile allows for a scope.
    pub fn max_sensitivity_for(&self, scope: &MemoryScope) -> Option<SensitivityLevel> {
        self.rules
            .iter()
            .filter(|r| r.scope == *scope)
            .map(|r| r.max_sensitivity)
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::MemoryScope;

    #[test]
    fn empty_profile_denies_all() {
        let profile = MemoryAccessProfile::new();
        let scope = MemoryScope::Agent {
            agent_id: "a1".into(),
        };
        assert!(!profile.can_read(&scope));
        assert!(!profile.can_write(&scope));
    }

    #[test]
    fn profile_with_rule() {
        let mut profile = MemoryAccessProfile::new();
        let scope = MemoryScope::Agent {
            agent_id: "a1".into(),
        };
        profile.add_rule(MemoryAccessRule {
            scope: scope.clone(),
            read: true,
            write: false,
            max_sensitivity: SensitivityLevel::Internal,
        });
        assert!(profile.can_read(&scope));
        assert!(!profile.can_write(&scope));
        assert_eq!(
            profile.max_sensitivity_for(&scope),
            Some(SensitivityLevel::Internal)
        );
    }
}
