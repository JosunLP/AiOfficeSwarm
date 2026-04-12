//! Persistent storage for learning outputs and their lifecycle.

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use swarm_core::error::SwarmResult;

use crate::output::{LearningOutput, LearningRuleId, LearningStatus};
use crate::scope::LearningScope;

fn scope_matches(output: &LearningOutput, scope: &LearningScope) -> bool {
    if matches!(scope, LearningScope::Global) {
        return true;
    }

    if &output.scope == scope {
        return true;
    }

    // Backward compatibility for records written before `LearningOutput.scope`
    // existed: legacy agent/tenant identifiers are only consulted when the
    // stored primary scope still reads as the default global value.
    match scope {
        LearningScope::Agent { agent_id } => {
            matches!(output.scope, LearningScope::Global)
                && output.agent_id.as_deref() == Some(agent_id.as_str())
        }
        LearningScope::Tenant { tenant_id } => {
            matches!(output.scope, LearningScope::Global)
                && output.tenant_id.as_deref() == Some(tenant_id.as_str())
        }
        LearningScope::Team { .. } | LearningScope::Workflow { .. } | LearningScope::Global => {
            false
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LearningStoreDocument {
    outputs: Vec<LearningOutput>,
}

/// JSON file-backed implementation of [`LearningStore`] for cross-process
/// approval queue persistence.
pub struct FileLearningStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileLearningStore {
    /// Create a file-backed learning store rooted at the provided JSON file.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// Return the configured storage path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load_document(&self) -> SwarmResult<LearningStoreDocument> {
        if !self.path.exists() {
            return Ok(LearningStoreDocument::default());
        }

        let content = std::fs::read_to_string(&self.path).map_err(|error| {
            swarm_core::error::SwarmError::Internal {
                reason: format!(
                    "failed to read learning store '{}': {}",
                    self.path.display(),
                    error
                ),
            }
        })?;

        if content.trim().is_empty() {
            return Ok(LearningStoreDocument::default());
        }

        serde_json::from_str(&content).map_err(|error| swarm_core::error::SwarmError::Internal {
            reason: format!(
                "failed to parse learning store '{}': {}",
                self.path.display(),
                error
            ),
        })
    }

    fn save_document(&self, document: &LearningStoreDocument) -> SwarmResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                swarm_core::error::SwarmError::Internal {
                    reason: format!(
                        "failed to create learning store directory '{}': {}",
                        parent.display(),
                        error
                    ),
                }
            })?;
        }

        let payload = serde_json::to_string_pretty(document).map_err(|error| {
            swarm_core::error::SwarmError::Internal {
                reason: format!("failed to serialise learning store: {}", error),
            }
        })?;
        let temp_path = temporary_store_path(&self.path);

        std::fs::write(&temp_path, payload).map_err(|error| {
            swarm_core::error::SwarmError::Internal {
                reason: format!(
                    "failed to write learning store temp file '{}': {}",
                    temp_path.display(),
                    error
                ),
            }
        })?;

        match std::fs::rename(&temp_path, &self.path) {
            Ok(()) => Ok(()),
            Err(rename_error) => match std::fs::remove_file(&self.path) {
                Ok(()) => {
                    std::fs::rename(&temp_path, &self.path).map_err(|error| {
                        swarm_core::error::SwarmError::Internal {
                            reason: format!(
                                "failed to finalise learning store '{}': {}",
                                self.path.display(),
                                error
                            ),
                        }
                    })?;
                    Ok(())
                }
                Err(remove_error) if remove_error.kind() == std::io::ErrorKind::NotFound => {
                    Err(swarm_core::error::SwarmError::Internal {
                        reason: format!(
                            "failed to rename learning store temp file into '{}': {}",
                            self.path.display(),
                            rename_error
                        ),
                    })
                }
                Err(remove_error) => Err(swarm_core::error::SwarmError::Internal {
                    reason: format!(
                        "failed to replace learning store '{}': {}; cleanup error: {}",
                        self.path.display(),
                        rename_error,
                        remove_error
                    ),
                }),
            },
        }
    }
}

#[async_trait]
impl LearningStore for FileLearningStore {
    async fn record(&self, output: LearningOutput) -> SwarmResult<()> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        match document
            .outputs
            .iter_mut()
            .find(|existing| existing.id == output.id)
        {
            Some(existing) => *existing = output,
            None => document.outputs.push(output),
        }
        self.save_document(&document)
    }

    async fn list_pending_approvals(
        &self,
        scope: &LearningScope,
    ) -> SwarmResult<Vec<LearningOutput>> {
        let _guard = self.lock.lock().await;
        let mut outputs: Vec<_> = self
            .load_document()?
            .outputs
            .into_iter()
            .filter(|output| {
                output.status == LearningStatus::PendingApproval && scope_matches(output, scope)
            })
            .collect();
        outputs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(outputs)
    }

    async fn approve(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        let output = document
            .outputs
            .iter_mut()
            .find(|output| output.id == *id)
            .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                reason: format!("Learning output {} not found", id),
            })?;
        output.status = LearningStatus::Applied;
        output.applied_at = Some(Utc::now());
        self.save_document(&document)
    }

    async fn reject(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        let output = document
            .outputs
            .iter_mut()
            .find(|output| output.id == *id)
            .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                reason: format!("Learning output {} not found", id),
            })?;
        output.status = LearningStatus::Rejected;
        self.save_document(&document)
    }

    async fn rollback(&self, id: &LearningRuleId) -> SwarmResult<()> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        let output = document
            .outputs
            .iter_mut()
            .find(|output| output.id == *id)
            .ok_or_else(|| swarm_core::error::SwarmError::Internal {
                reason: format!("Learning output {} not found", id),
            })?;
        output.status = LearningStatus::RolledBack;
        self.save_document(&document)
    }

    async fn get(&self, id: &LearningRuleId) -> SwarmResult<Option<LearningOutput>> {
        let _guard = self.lock.lock().await;
        Ok(self
            .load_document()?
            .outputs
            .into_iter()
            .find(|output| output.id == *id))
    }

    async fn count(&self, scope: &LearningScope) -> SwarmResult<u64> {
        let _guard = self.lock.lock().await;
        Ok(self
            .load_document()?
            .outputs
            .into_iter()
            .filter(|output| scope_matches(output, scope))
            .count() as u64)
    }
}

fn temporary_store_path(path: &Path) -> PathBuf {
    let mut temp_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "learning-store".into());
    temp_name.push_str(".tmp");
    path.with_file_name(temp_name)
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
        Ok(self
            .outputs
            .iter()
            .filter(|entry| {
                let output = entry.value();
                output.status == crate::output::LearningStatus::PendingApproval
                    && scope_matches(output, scope)
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

    async fn count(&self, scope: &LearningScope) -> SwarmResult<u64> {
        Ok(self
            .outputs
            .iter()
            .filter(|entry| scope_matches(entry.value(), scope))
            .count() as u64)
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

    #[tokio::test]
    async fn file_store_persists_outputs_across_instances() {
        let path =
            std::env::temp_dir().join(format!("swarm-learning-{}.json", uuid::Uuid::new_v4()));
        let first_store = FileLearningStore::new(&path);
        let mut output = LearningOutput::requires_review(
            LearningCategory::ConfigurationEvolution,
            "Persist me",
            serde_json::json!({"timeout": 900}),
            serde_json::json!({"source": "test"}),
        );
        output.tenant_id = Some("tenant-a".into());
        let output_id = output.id;

        first_store.record(output).await.unwrap();

        let second_store = FileLearningStore::new(&path);
        let pending = second_store
            .list_pending_approvals(&LearningScope::Tenant {
                tenant_id: "tenant-a".into(),
            })
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, output_id);

        second_store.approve(&output_id).await.unwrap();
        let approved = second_store.get(&output_id).await.unwrap().unwrap();
        assert_eq!(approved.status, LearningStatus::Applied);

        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn filters_team_and_workflow_outputs_by_primary_scope() {
        let store = InMemoryLearningStore::new();

        let mut team_output = LearningOutput::requires_review(
            LearningCategory::PatternExtraction,
            "Persist team pattern",
            serde_json::json!({"task_count": 3}),
            serde_json::json!({"source": "team-run"}),
        );
        team_output.set_scope(LearningScope::Team {
            team_id: "team-a".into(),
        });
        store.record(team_output).await.unwrap();

        let mut workflow_output = LearningOutput::requires_review(
            LearningCategory::PlanTemplate,
            "Persist workflow template",
            serde_json::json!({"steps": 4}),
            serde_json::json!({"source": "workflow-run"}),
        );
        workflow_output.set_scope(LearningScope::Workflow {
            workflow_id: "workflow-a".into(),
        });
        store.record(workflow_output).await.unwrap();

        let team_pending = store
            .list_pending_approvals(&LearningScope::Team {
                team_id: "team-a".into(),
            })
            .await
            .unwrap();
        assert_eq!(team_pending.len(), 1);
        assert_eq!(team_pending[0].scope.label(), "team");

        let workflow_pending = store
            .list_pending_approvals(&LearningScope::Workflow {
                workflow_id: "workflow-a".into(),
            })
            .await
            .unwrap();
        assert_eq!(workflow_pending.len(), 1);
        assert_eq!(workflow_pending[0].scope.label(), "workflow");
    }
}
