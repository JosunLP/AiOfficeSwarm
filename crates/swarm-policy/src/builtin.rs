//! Built-in policy implementations for common use cases.
//!
//! These policies can be used directly or as templates for custom policies.

use async_trait::async_trait;
use std::collections::HashSet;

use swarm_core::{
    error::SwarmResult,
    identity::PolicyId,
    policy::{Policy, PolicyContext, PolicyOutcome},
};

/// A policy that explicitly allows every action.
///
/// **Warning**: Use only in development or as a fallback layer where all
/// restrictive policies have already been evaluated.
pub struct AllowAllPolicy {
    id: PolicyId,
    name: String,
}

impl AllowAllPolicy {
    /// Create an allow-all policy with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: PolicyId::new(),
            name: name.into(),
        }
    }
}

#[async_trait]
impl Policy for AllowAllPolicy {
    fn id(&self) -> PolicyId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn evaluate(&self, _context: &PolicyContext) -> SwarmResult<PolicyOutcome> {
        Ok(PolicyOutcome::Allow)
    }
}

/// A policy that explicitly denies every action.
///
/// Useful as a low-priority "deny-all" backstop, or in tests.
pub struct DenyAllPolicy {
    id: PolicyId,
    name: String,
    reason: String,
}

impl DenyAllPolicy {
    /// Create a deny-all policy with the given name and denial reason.
    pub fn new(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: PolicyId::new(),
            name: name.into(),
            reason: reason.into(),
        }
    }
}

#[async_trait]
impl Policy for DenyAllPolicy {
    fn id(&self) -> PolicyId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn evaluate(&self, _context: &PolicyContext) -> SwarmResult<PolicyOutcome> {
        Ok(PolicyOutcome::Deny {
            reason: self.reason.clone(),
        })
    }
}

/// A policy that allows only a specific set of named actions.
///
/// Any action not in the allowlist results in a `Deny`. This is useful for
/// creating minimal-privilege policies for specific agent types.
pub struct ActionAllowlistPolicy {
    id: PolicyId,
    name: String,
    allowed_actions: HashSet<String>,
}

impl ActionAllowlistPolicy {
    /// Create a policy allowing the specified actions.
    pub fn new(name: impl Into<String>, actions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            id: PolicyId::new(),
            name: name.into(),
            allowed_actions: actions.into_iter().map(|a| a.into()).collect(),
        }
    }
}

#[async_trait]
impl Policy for ActionAllowlistPolicy {
    fn id(&self) -> PolicyId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn evaluate(&self, context: &PolicyContext) -> SwarmResult<PolicyOutcome> {
        if self.allowed_actions.contains(&context.action) {
            Ok(PolicyOutcome::Allow)
        } else {
            Ok(PolicyOutcome::Deny {
                reason: format!(
                    "action '{}' is not in the allowlist for policy '{}'",
                    context.action, self.name
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allow_all_always_allows() {
        let p = AllowAllPolicy::new("test");
        let ctx = PolicyContext::new("anything", "anyone", "resource");
        let outcome = p.evaluate(&ctx).await.unwrap();
        assert_eq!(outcome, PolicyOutcome::Allow);
    }

    #[tokio::test]
    async fn deny_all_always_denies() {
        let p = DenyAllPolicy::new("test", "blocked");
        let ctx = PolicyContext::new("anything", "anyone", "resource");
        let outcome = p.evaluate(&ctx).await.unwrap();
        assert!(matches!(outcome, PolicyOutcome::Deny { .. }));
    }

    #[tokio::test]
    async fn action_allowlist_permits_listed_action() {
        let p = ActionAllowlistPolicy::new("test", ["create_task", "read_task"]);
        let ctx = PolicyContext::new("create_task", "agent", "task-queue");
        let outcome = p.evaluate(&ctx).await.unwrap();
        assert_eq!(outcome, PolicyOutcome::Allow);
    }

    #[tokio::test]
    async fn action_allowlist_denies_unlisted_action() {
        let p = ActionAllowlistPolicy::new("test", ["create_task"]);
        let ctx = PolicyContext::new("delete_agent", "agent", "agent-registry");
        let outcome = p.evaluate(&ctx).await.unwrap();
        assert!(matches!(outcome, PolicyOutcome::Deny { .. }));
    }
}
