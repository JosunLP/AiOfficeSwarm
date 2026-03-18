//! The central policy evaluation engine.
//!
//! The [`PolicyEngine`] holds a collection of [`Policy`] trait objects ordered
//! by priority. When an action is evaluated, the engine calls each policy in
//! priority order and stops at the first explicit `Allow` or `Deny` decision.
//! If all policies `Abstain`, the engine applies a configurable default
//! decision.

use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::PolicyId,
    policy::{Policy, PolicyContext, PolicyDecision, PolicyOutcome},
};

/// Default decision when all registered policies abstain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultDecision {
    /// Allow the action if no policy explicitly denies it (permissive).
    Allow,
    /// Deny the action if no policy explicitly allows it (restrictive, recommended).
    Deny,
}

/// The async, priority-ordered policy evaluation engine.
///
/// Thread-safe: uses an async `RwLock` for the policy list so that reads
/// (evaluations) are concurrent and writes (registrations) are serialized.
#[derive(Clone)]
pub struct PolicyEngine {
    policies: Arc<RwLock<Vec<Arc<dyn Policy>>>>,
    default_decision: DefaultDecision,
    default_deny_policy_id: PolicyId,
}

impl PolicyEngine {
    fn default_deny_policy_id() -> PolicyId {
        static DEFAULT_DENY_POLICY_ID: OnceLock<PolicyId> = OnceLock::new();

        *DEFAULT_DENY_POLICY_ID.get_or_init(|| {
            // Fixed across processes so deny-by-default audit/log events remain
            // correlatable even when no user-defined policy supplied the denial.
            // The `d3f1` suffix is a mnemonic for the default-deny fallback.
            "00000000-0000-0000-0000-00000000d3f1"
                .parse()
                .expect("default deny policy ID must be a valid UUID")
        })
    }

    /// Create a new engine with the given default decision.
    pub fn new(default_decision: DefaultDecision) -> Self {
        Self {
            policies: Arc::new(RwLock::new(Vec::new())),
            default_decision,
            default_deny_policy_id: Self::default_deny_policy_id(),
        }
    }

    /// Create a new engine with a **deny-by-default** posture (recommended).
    pub fn deny_by_default() -> Self {
        Self::new(DefaultDecision::Deny)
    }

    /// Create a new engine with an **allow-by-default** posture.
    ///
    /// Use only for development or when all policies are explicitly configured.
    pub fn allow_by_default() -> Self {
        Self::new(DefaultDecision::Allow)
    }

    /// Register a policy. Policies are kept sorted by descending priority.
    pub async fn register(&self, policy: Arc<dyn Policy>) {
        let mut policies = self.policies.write().await;
        policies.push(policy);
        // Sort descending by priority so the highest-priority policy is first.
        policies.sort_by_key(|policy| std::cmp::Reverse(policy.priority()));
        tracing::debug!("Policy registered; total policies: {}", policies.len());
    }

    /// Remove a policy by ID.
    pub async fn unregister(&self, id: &PolicyId) {
        let mut policies = self.policies.write().await;
        policies.retain(|p| &p.id() != id);
    }

    /// Evaluate all registered policies for the given context.
    ///
    /// Returns the first explicit decision in descending policy priority order.
    ///
    /// - [`PolicyDecision::Allowed`] if the first non-abstaining policy allows.
    /// - [`PolicyDecision::Denied`] if the first non-abstaining policy denies.
    /// - The configured default decision if all policies abstain.
    pub async fn evaluate(&self, context: &PolicyContext) -> SwarmResult<PolicyDecision> {
        let policies: Vec<_> = {
            let policies = self.policies.read().await;
            policies.iter().cloned().collect()
        };

        for policy in &policies {
            match policy.evaluate(context).await? {
                PolicyOutcome::Allow => {
                    tracing::debug!(
                        policy = policy.name(),
                        action = %context.action,
                        "Policy allowed action"
                    );
                    return Ok(PolicyDecision::Allowed);
                }
                PolicyOutcome::Deny { reason } => {
                    tracing::warn!(
                        policy = policy.name(),
                        action = %context.action,
                        subject = %context.subject,
                        reason = %reason,
                        "Policy denied action"
                    );
                    return Ok(PolicyDecision::Denied {
                        policy_id: policy.id(),
                        reason,
                    });
                }
                PolicyOutcome::Abstain => continue,
            }
        }

        // All policies abstained — apply default.
        match self.default_decision {
            DefaultDecision::Allow => Ok(PolicyDecision::Allowed),
            DefaultDecision::Deny => Ok(PolicyDecision::Denied {
                policy_id: self.default_deny_policy_id,
                reason: "no policy explicitly allowed this action (deny-by-default)".into(),
            }),
        }
    }

    /// Convenience method: evaluate and return `Ok(())` if allowed, or an error
    /// if denied.
    pub async fn enforce(&self, context: &PolicyContext) -> SwarmResult<()> {
        match self.evaluate(context).await? {
            PolicyDecision::Allowed => Ok(()),
            PolicyDecision::Denied { policy_id, reason } => Err(SwarmError::PolicyViolation {
                policy_id,
                action: context.action.clone(),
                reason,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{AllowAllPolicy, DenyAllPolicy};
    use async_trait::async_trait;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Notify;

    struct BlockingPolicy {
        id: PolicyId,
        name: String,
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl Policy for BlockingPolicy {
        fn id(&self) -> PolicyId {
            self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        async fn evaluate(&self, _context: &PolicyContext) -> SwarmResult<PolicyOutcome> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(PolicyOutcome::Abstain)
        }

        fn priority(&self) -> i32 {
            100
        }
    }

    #[tokio::test]
    async fn allow_by_default_when_no_policies() {
        let engine = PolicyEngine::allow_by_default();
        let ctx = PolicyContext::new("create_task", "agent-1", "task-queue");
        let decision = engine.evaluate(&ctx).await.unwrap();
        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn deny_by_default_when_no_policies() {
        let engine = PolicyEngine::deny_by_default();
        let ctx = PolicyContext::new("create_task", "agent-1", "task-queue");
        let decision = engine.evaluate(&ctx).await.unwrap();
        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn deny_all_policy_overrides_default() {
        let engine = PolicyEngine::allow_by_default();
        engine
            .register(Arc::new(DenyAllPolicy::new("deny-all", "testing")))
            .await;
        let ctx = PolicyContext::new("any-action", "anyone", "anything");
        let decision = engine.evaluate(&ctx).await.unwrap();
        assert!(decision.is_denied());
    }

    #[tokio::test]
    async fn allow_all_policy_overrides_deny_default() {
        let engine = PolicyEngine::deny_by_default();
        engine
            .register(Arc::new(AllowAllPolicy::new("allow-all")))
            .await;
        let ctx = PolicyContext::new("any-action", "anyone", "anything");
        let decision = engine.evaluate(&ctx).await.unwrap();
        assert!(decision.is_allowed());
    }

    #[tokio::test]
    async fn deny_by_default_uses_stable_policy_id() {
        let engine = PolicyEngine::deny_by_default();
        let ctx = PolicyContext::new("create_task", "agent-1", "task-queue");
        let first = match engine.evaluate(&ctx).await.unwrap() {
            PolicyDecision::Denied { policy_id, .. } => policy_id,
            PolicyDecision::Allowed => panic!("expected deny-by-default to deny"),
        };
        let second = match engine.evaluate(&ctx).await.unwrap() {
            PolicyDecision::Denied { policy_id, .. } => policy_id,
            PolicyDecision::Allowed => panic!("expected deny-by-default to deny"),
        };
        assert_eq!(first, second);
        assert_eq!(
            first,
            "00000000-0000-0000-0000-00000000d3f1"
                .parse()
                .expect("test UUID should parse")
        );
    }

    #[tokio::test]
    async fn deny_by_default_uses_same_policy_id_across_engines() {
        let first = PolicyEngine::deny_by_default();
        let second = PolicyEngine::deny_by_default();
        let ctx = PolicyContext::new("create_task", "agent-1", "task-queue");

        let first_id = match first.evaluate(&ctx).await.unwrap() {
            PolicyDecision::Denied { policy_id, .. } => policy_id,
            PolicyDecision::Allowed => panic!("expected deny-by-default to deny"),
        };
        let second_id = match second.evaluate(&ctx).await.unwrap() {
            PolicyDecision::Denied { policy_id, .. } => policy_id,
            PolicyDecision::Allowed => panic!("expected deny-by-default to deny"),
        };

        assert_eq!(first_id, second_id);
    }

    #[tokio::test]
    async fn enforce_returns_error_on_deny() {
        let engine = PolicyEngine::allow_by_default();
        engine
            .register(Arc::new(DenyAllPolicy::new("deny-all", "blocked")))
            .await;
        let ctx = PolicyContext::new("delete_agent", "user", "agent");
        assert!(engine.enforce(&ctx).await.is_err());
    }

    #[tokio::test]
    async fn register_does_not_wait_for_inflight_policy_evaluation() {
        let engine = PolicyEngine::allow_by_default();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());

        engine
            .register(Arc::new(BlockingPolicy {
                id: "00000000-0000-0000-0000-000000000111"
                    .parse()
                    .expect("test UUID should parse"),
                name: "blocking-policy".into(),
                started: Arc::clone(&started),
                release: Arc::clone(&release),
            }))
            .await;

        let eval_engine = engine.clone();
        let ctx = PolicyContext::new("inspect", "agent-1", "resource-1");
        let eval_task = tokio::spawn(async move { eval_engine.evaluate(&ctx).await });

        started.notified().await;

        tokio::time::timeout(
            Duration::from_millis(200),
            engine.register(Arc::new(AllowAllPolicy::new("allow-all-fast"))),
        )
        .await
        .expect("register should not wait for in-flight evaluation");

        let policies = engine.policies.read().await;
        assert!(policies
            .iter()
            .any(|policy| policy.name() == "allow-all-fast"));
        drop(policies);

        release.notify_one();
        eval_task
            .await
            .expect("evaluation task should finish")
            .expect("evaluation should succeed");
    }
}
