//! Bridge between role learning policies and the learning subsystem.
//!
//! Converts [`RoleLearningPolicy`] into a [`LearningScopeConfig`] that
//! the learning subsystem can enforce.

use swarm_learning::scope::{LearningScope, LearningScopeConfig};

use crate::model::RoleLearningPolicy;

/// Build a [`LearningScopeConfig`] from a role's learning policy and the
/// agent ID that will be bound to this role.
pub fn scope_config_from_role(policy: &RoleLearningPolicy, agent_id: &str) -> LearningScopeConfig {
    let scope = LearningScope::Agent {
        agent_id: agent_id.to_string(),
    };

    if !policy.enabled {
        return LearningScopeConfig::disabled(scope);
    }

    if policy.require_approval {
        LearningScopeConfig::with_approval(scope)
    } else {
        LearningScopeConfig::enabled(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_policy() {
        let policy = RoleLearningPolicy {
            enabled: false,
            require_approval: false,
            allowed_categories: vec![],
            denied_categories: vec![],
        };
        let config = scope_config_from_role(&policy, "agent-1");
        assert!(!config.enabled);
    }

    #[test]
    fn enabled_with_approval() {
        let policy = RoleLearningPolicy {
            enabled: true,
            require_approval: true,
            allowed_categories: vec!["workflow".into()],
            denied_categories: vec![],
        };
        let config = scope_config_from_role(&policy, "agent-1");
        assert!(config.enabled);
        assert!(config.require_approval);
    }
}
