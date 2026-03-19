//! Bridge between role memory policies and the memory subsystem.
//!
//! Converts [`RoleMemoryPolicy`] into a [`MemoryAccessProfile`] with
//! appropriate scope rules.

use swarm_memory::access::{MemoryAccessProfile, MemoryAccessRule};
use swarm_memory::entry::{MemoryScope, SensitivityLevel};

use crate::model::RoleMemoryPolicy;

/// Build a [`MemoryAccessProfile`] from a role's memory policy and the
/// agent ID that will be bound to this role.
pub fn access_profile_from_role(policy: &RoleMemoryPolicy, agent_id: &str) -> MemoryAccessProfile {
    let max_sens = parse_sensitivity(policy.max_sensitivity.as_deref());

    let mut profile = MemoryAccessProfile::new();

    // Always grant the agent read/write to its own agent scope.
    profile.add_rule(MemoryAccessRule {
        scope: MemoryScope::Agent {
            agent_id: agent_id.to_string(),
        },
        read: true,
        write: true,
        max_sensitivity: max_sens,
    });

    // Map readable scopes.
    for scope_label in &policy.readable_scopes {
        if let Some(scope) = label_to_scope(scope_label, agent_id) {
            // Only add if not already covered by the agent scope above.
            profile.add_rule(MemoryAccessRule {
                scope,
                read: true,
                write: false,
                max_sensitivity: max_sens,
            });
        }
    }

    // Map writable scopes.
    for scope_label in &policy.writable_scopes {
        if let Some(scope) = label_to_scope(scope_label, agent_id) {
            profile.add_rule(MemoryAccessRule {
                scope,
                read: true, // Write implies read.
                write: true,
                max_sensitivity: max_sens,
            });
        }
    }

    profile
}

fn parse_sensitivity(s: Option<&str>) -> SensitivityLevel {
    match s.map(|v| v.to_lowercase()).as_deref() {
        Some("public") => SensitivityLevel::Public,
        Some("internal") => SensitivityLevel::Internal,
        Some("confidential") => SensitivityLevel::Confidential,
        Some("restricted") => SensitivityLevel::Restricted,
        _ => SensitivityLevel::Internal,
    }
}

fn label_to_scope(label: &str, agent_id: &str) -> Option<MemoryScope> {
    let lower = label.to_lowercase();
    match lower.as_str() {
        "session" => Some(MemoryScope::Session {
            session_id: "*".into(),
        }),
        "task" => Some(MemoryScope::Task {
            task_id: "*".into(),
        }),
        "agent" => Some(MemoryScope::Agent {
            agent_id: agent_id.to_string(),
        }),
        "team" => Some(MemoryScope::Team {
            team_id: "*".into(),
        }),
        "tenant" => Some(MemoryScope::Tenant {
            tenant_id: "*".into(),
        }),
        "long_term" | "longterm" | "persistent" => Some(MemoryScope::LongTerm {
            owner_id: agent_id.to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_policy_gives_agent_scope_only() {
        let policy = RoleMemoryPolicy::default();
        let profile = access_profile_from_role(&policy, "agent-1");
        assert_eq!(profile.rules.len(), 1);
        assert!(profile.can_read(&MemoryScope::Agent {
            agent_id: "agent-1".into()
        }));
    }

    #[test]
    fn readable_scopes_mapped() {
        let policy = RoleMemoryPolicy {
            readable_scopes: vec!["team".into(), "tenant".into()],
            writable_scopes: vec![],
            max_sensitivity: Some("confidential".into()),
            retention_hint: None,
        };
        let profile = access_profile_from_role(&policy, "agent-1");
        assert!(profile.can_read(&MemoryScope::Team {
            team_id: "*".into()
        }));
        assert!(!profile.can_write(&MemoryScope::Team {
            team_id: "*".into()
        }));
    }
}
