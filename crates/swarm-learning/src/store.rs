//! Persistent storage for learning outputs and their lifecycle.

use async_trait::async_trait;

use swarm_core::error::SwarmResult;

use crate::output::{LearningOutput, LearningRuleId};
use crate::scope::LearningScope;

/// Storage backend for learning outputs and their approval lifecycle.
///
/// Implement this trait to provide persistent storage for learning state
/// (in-memory for testing, database for production, etc.).
#[async_trait]
pub trait LearningStore: Send + Sync {
    /// Record a new learning output.
    async fn record(&self, output: LearningOutput) -> SwarmResult<()>;

    /// List all outputs pending approval for the given scope.
    async fn list_pending_approvals(
        &self,
        scope: &LearningScope,
    ) -> SwarmResult<Vec<LearningOutput>>;

    /// Approve a pending output.
    async fn approve(&self, id: &LearningRuleId) -> SwarmResult<()>;

    /// Reject a pending output.
    async fn reject(&self, id: &LearningRuleId) -> SwarmResult<()>;

    /// Roll back a previously applied output.
    async fn rollback(&self, id: &LearningRuleId) -> SwarmResult<()>;

    /// Retrieve a single output by ID.
    async fn get(&self, id: &LearningRuleId) -> SwarmResult<Option<LearningOutput>>;

    /// Return the total number of recorded outputs for a scope.
    async fn count(&self, scope: &LearningScope) -> SwarmResult<u64>;
}

/// In-memory implementation of [`LearningStore`] for testing.
pub struct InMemoryLearningStore {
    outputs: dashmap::DashMap<LearningRuleId, LearningOutput>,
}

impl Default for InMemoryLearningStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryLearningStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            outputs: dashmap::DashMap::new(),
        }
    }
}

#[async_trait]
impl LearningStore for InMemoryLearningStore {
    async fn record(&self, output: LearningOutput) -> SwarmResult<()> {
        self.outputs.insert(output.id, output);
        Ok(())
    }

    async fn list_pending_approvals(
        &self,
        scope: &LearningScope,
    ) -> SwarmResult<Vec<LearningOutput>> {
        let scope_label = scope.label();
        Ok(self
            .outputs
            .iter()
            .filter(|e| {
                let o = e.value();
                o.status == crate::output::LearningStatus::PendingApproval
                    && match scope {
                        LearningScope::Agent { agent_id } => {
                            o.agent_id.as_deref() == Some(agent_id.as_str())
                        }
                        LearningScope::Tenant { tenant_id } => {
                            o.tenant_id.as_deref() == Some(tenant_id.as_str())
                        }
                        LearningScope::Global => true,
                        _ => {
                            let _ = scope_label;
                            true
                        }
                    }
            })
            .map(|e| e.value().clone())
            .collect())
    }

    async fn approve(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let mut entry =
            self.outputs
                .get_mut(id)
                .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                    reason: format!("Learning output {} not found", id),
                })?;
        entry.status = crate::output::LearningStatus::Applied;
        entry.applied_at = Some(chrono::Utc::now());
        Ok(())
    }

    async fn reject(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let mut entry =
            self.outputs
                .get_mut(id)
                .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                    reason: format!("Learning output {} not found", id),
                })?;
        entry.status = crate::output::LearningStatus::Rejected;
        Ok(())
    }

    async fn rollback(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let mut entry =
            self.outputs
                .get_mut(id)
                .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                    reason: format!("Learning output {} not found", id),
                })?;
        entry.status = crate::output::LearningStatus::RolledBack;
        Ok(())
    }

    async fn get(&self, id: &LearningRuleId) -> SwarmResult<Option<LearningOutput>> {
        Ok(self.outputs.get(id).map(|e| e.value().clone()))
    }

    async fn count(&self, _scope: &LearningScope) -> SwarmResult<u64> {
        Ok(self.outputs.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{LearningCategory, LearningOutput, LearningStatus};

    #[tokio::test]
    async fn record_and_approve() {
        let store = InMemoryLearningStore::new();
        let output = LearningOutput::requires_review(
            LearningCategory::ConfigurationEvolution,
            "Increase timeout",
            serde_json::json!({"timeout": 600}),
            serde_json::json!({"reason": "timeouts observed"}),
        );
        let id = output.id;
        store.record(output).await.unwrap();

        let pending = store
            .list_pending_approvals(&LearningScope::Global)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);

        store.approve(&id).await.unwrap();
        let approved = store.get(&id).await.unwrap().unwrap();
        assert_eq!(approved.status, LearningStatus::Applied);
    }

    #[tokio::test]
    async fn rollback() {
        let store = InMemoryLearningStore::new();
        let output = LearningOutput::auto(
            LearningCategory::PreferenceAdaptation,
            "Test",
            serde_json::json!({}),
        );
        let id = output.id;
        store.record(output).await.unwrap();
        store.approve(&id).await.unwrap();
        store.rollback(&id).await.unwrap();

        let rolled_back = store.get(&id).await.unwrap().unwrap();
        assert_eq!(rolled_back.status, LearningStatus::RolledBack);
    }
}
