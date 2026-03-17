//! Policy engine contracts.
//!
//! A [`Policy`] is a named rule that the orchestrator evaluates before
//! performing sensitive operations (scheduling, delegation, plugin execution,
//! external calls, etc.). Policies provide the enforcement layer for access
//! control, resource quotas, and custom business rules.
//!
//! ## Design
//! Policies are trait objects so that new policy types can be added via the
//! plugin system without modifying the core crate. The engine evaluates a list
//! of policies in priority order and returns the first explicit
//! [`PolicyDecision`] (allow or deny); if all policies abstain, the configured
//! default decision is applied.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SwarmResult;
use crate::identity::PolicyId;

/// The outcome of a single policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyOutcome {
    /// The policy explicitly permits the action.
    Allow,
    /// The policy explicitly denies the action.
    Deny {
        /// Human-readable explanation.
        reason: String,
    },
    /// The policy does not have an opinion; evaluation should continue.
    Abstain,
}

/// The aggregated decision after evaluating all applicable policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// All policies allowed (or abstained from) the action.
    Allowed,
    /// At least one policy denied the action.
    Denied {
        /// The policy that triggered the denial.
        policy_id: PolicyId,
        /// The denial reason.
        reason: String,
    },
}

impl PolicyDecision {
    /// Returns `true` if the decision is [`PolicyDecision::Allowed`].
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allowed)
    }

    /// Returns `true` if the decision is [`PolicyDecision::Denied`].
    pub fn is_denied(&self) -> bool {
        matches!(self, PolicyDecision::Denied { .. })
    }
}

/// The evaluation context provided to a policy during evaluation.
///
/// Contains all information a policy needs to make a decision without
/// requiring access to mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyContext {
    /// The action being requested (e.g., `"schedule_task"`, `"invoke_plugin"`).
    pub action: String,
    /// Identifier of the subject requesting the action (agent ID, user, etc.).
    pub subject: String,
    /// The resource being acted upon.
    pub resource: String,
    /// Additional context attributes (free-form JSON).
    pub attributes: serde_json::Value,
}

impl PolicyContext {
    /// Create a minimal policy context.
    pub fn new(
        action: impl Into<String>,
        subject: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self {
            action: action.into(),
            subject: subject.into(),
            resource: resource.into(),
            attributes: serde_json::Value::Null,
        }
    }
}

/// A named, versioned rule evaluated by the policy engine.
///
/// Implement this trait to create custom policy logic. Policies are registered
/// with the policy engine and evaluated in priority order.
#[async_trait]
pub trait Policy: Send + Sync {
    /// The unique identifier for this policy instance.
    fn id(&self) -> PolicyId;

    /// A human-readable name for logging and audit purposes.
    fn name(&self) -> &str;

    /// Evaluate the policy against the given context and return an outcome.
    async fn evaluate(&self, context: &PolicyContext) -> SwarmResult<PolicyOutcome>;

    /// The evaluation priority. Higher values are evaluated first.
    /// Default is `0` (lowest priority).
    fn priority(&self) -> i32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_decision_is_allowed() {
        assert!(PolicyDecision::Allowed.is_allowed());
        assert!(!PolicyDecision::Allowed.is_denied());
    }

    #[test]
    fn policy_decision_is_denied() {
        let denied = PolicyDecision::Denied {
            policy_id: PolicyId::new(),
            reason: "quota exceeded".into(),
        };
        assert!(denied.is_denied());
        assert!(!denied.is_allowed());
    }

    #[test]
    fn policy_context_new() {
        let ctx = PolicyContext::new("schedule_task", "agent-1", "task-queue");
        assert_eq!(ctx.action, "schedule_task");
    }
}
