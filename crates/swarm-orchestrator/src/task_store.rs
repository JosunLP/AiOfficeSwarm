//! Persistent storage for task snapshots across CLI and embedding sessions.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::TaskId,
    task::{Task, TaskStatus},
    types::now,
};

/// Storage backend for cross-process task snapshots.
#[async_trait]
pub trait TaskStore: Send + Sync {
    /// Record or replace a task snapshot.
    async fn record(&self, task: Task) -> SwarmResult<()>;

    /// List all known task snapshots.
    async fn list(&self) -> SwarmResult<Vec<Task>>;

    /// Retrieve a single task snapshot by ID.
    async fn get(&self, id: &TaskId) -> SwarmResult<Option<Task>>;

    /// Cancel a pending task snapshot.
    async fn cancel(&self, id: &TaskId, reason: Option<String>) -> SwarmResult<Task>;

    /// Return a retryable terminal task snapshot to the pending state.
    async fn retry(&self, id: &TaskId) -> SwarmResult<Task>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TaskStoreDocument {
    tasks: Vec<Task>,
}

/// JSON file-backed implementation of [`TaskStore`].
pub struct FileTaskStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileTaskStore {
    /// Create a file-backed task store rooted at the provided JSON file.
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

    fn load_document(&self) -> SwarmResult<TaskStoreDocument> {
        if !self.path.exists() {
            return Ok(TaskStoreDocument::default());
        }

        let content =
            std::fs::read_to_string(&self.path).map_err(|error| SwarmError::Internal {
                reason: format!(
                    "failed to read task store '{}': {}",
                    self.path.display(),
                    error
                ),
            })?;

        if content.trim().is_empty() {
            return Ok(TaskStoreDocument::default());
        }

        serde_json::from_str(&content).map_err(|error| SwarmError::Internal {
            reason: format!(
                "failed to parse task store '{}': {}",
                self.path.display(),
                error
            ),
        })
    }

    fn save_document(&self, document: &TaskStoreDocument) -> SwarmResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SwarmError::Internal {
                reason: format!(
                    "failed to create task store directory '{}': {}",
                    parent.display(),
                    error
                ),
            })?;
        }

        let payload =
            serde_json::to_string_pretty(document).map_err(|error| SwarmError::Internal {
                reason: format!("failed to serialize task store: {}", error),
            })?;
        let temp_path = temporary_store_path(&self.path);

        std::fs::write(&temp_path, payload).map_err(|error| SwarmError::Internal {
            reason: format!(
                "failed to write task store temp file '{}': {}",
                temp_path.display(),
                error
            ),
        })?;

        match std::fs::rename(&temp_path, &self.path) {
            Ok(()) => Ok(()),
            Err(rename_error) => match std::fs::remove_file(&self.path) {
                Ok(()) => {
                    std::fs::rename(&temp_path, &self.path).map_err(|error| {
                        SwarmError::Internal {
                            reason: format!(
                                "failed to finalise task store '{}': {}",
                                self.path.display(),
                                error
                            ),
                        }
                    })?;
                    Ok(())
                }
                Err(remove_error) if remove_error.kind() == std::io::ErrorKind::NotFound => {
                    Err(SwarmError::Internal {
                        reason: format!(
                            "failed to rename task store temp file into '{}': {}",
                            self.path.display(),
                            rename_error
                        ),
                    })
                }
                Err(remove_error) => Err(SwarmError::Internal {
                    reason: format!(
                        "failed to replace task store '{}': {}; cleanup error: {}",
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
impl TaskStore for FileTaskStore {
    async fn record(&self, task: Task) -> SwarmResult<()> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        match document
            .tasks
            .iter_mut()
            .find(|existing| existing.id == task.id)
        {
            Some(existing) => *existing = task,
            None => document.tasks.push(task),
        }
        self.save_document(&document)
    }

    async fn list(&self) -> SwarmResult<Vec<Task>> {
        let _guard = self.lock.lock().await;
        let mut tasks = self.load_document()?.tasks;
        tasks.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(tasks)
    }

    async fn get(&self, id: &TaskId) -> SwarmResult<Option<Task>> {
        let _guard = self.lock.lock().await;
        Ok(self
            .load_document()?
            .tasks
            .into_iter()
            .find(|task| task.id == *id))
    }

    async fn cancel(&self, id: &TaskId, reason: Option<String>) -> SwarmResult<Task> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        let task = document
            .tasks
            .iter_mut()
            .find(|task| task.id == *id)
            .ok_or(SwarmError::TaskNotFound { id: *id })?;

        if !matches!(task.status, TaskStatus::Pending) {
            return Err(SwarmError::InvalidTaskSpec {
                reason: format!(
                    "only pending tasks can be cancelled from the task store; got {}",
                    task.status.label()
                ),
            });
        }

        let cancelled_at = now();
        task.status = TaskStatus::Cancelled {
            cancelled_at,
            reason,
        };
        task.updated_at = cancelled_at;
        let snapshot = task.clone();
        self.save_document(&document)?;
        Ok(snapshot)
    }

    async fn retry(&self, id: &TaskId) -> SwarmResult<Task> {
        let _guard = self.lock.lock().await;
        let mut document = self.load_document()?;
        let task = document
            .tasks
            .iter_mut()
            .find(|task| task.id == *id)
            .ok_or(SwarmError::TaskNotFound { id: *id })?;

        task.retry().map_err(|status| SwarmError::InvalidTaskSpec {
            reason: format!(
                "only failed, cancelled, or timed out tasks can be retried from the task store; got {}",
                status.label()
            ),
        })?;

        let snapshot = task.clone();
        self.save_document(&document)?;
        Ok(snapshot)
    }
}

fn temporary_store_path(path: &Path) -> PathBuf {
    let mut temp_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "task-store".into());
    temp_name.push_str(".tmp");
    path.with_file_name(temp_name)
}

/// In-memory implementation of [`TaskStore`] for testing.
pub struct InMemoryTaskStore {
    tasks: dashmap::DashMap<TaskId, Task>,
}

impl Default for InMemoryTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTaskStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            tasks: dashmap::DashMap::new(),
        }
    }
}

#[async_trait]
impl TaskStore for InMemoryTaskStore {
    async fn record(&self, task: Task) -> SwarmResult<()> {
        self.tasks.insert(task.id, task);
        Ok(())
    }

    async fn list(&self) -> SwarmResult<Vec<Task>> {
        let mut tasks: Vec<_> = self
            .tasks
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        tasks.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(tasks)
    }

    async fn get(&self, id: &TaskId) -> SwarmResult<Option<Task>> {
        Ok(self.tasks.get(id).map(|entry| entry.value().clone()))
    }

    async fn cancel(&self, id: &TaskId, reason: Option<String>) -> SwarmResult<Task> {
        let mut entry = self
            .tasks
            .get_mut(id)
            .ok_or(SwarmError::TaskNotFound { id: *id })?;
        if !matches!(entry.status, TaskStatus::Pending) {
            return Err(SwarmError::InvalidTaskSpec {
                reason: format!(
                    "only pending tasks can be cancelled from the task store; got {}",
                    entry.status.label()
                ),
            });
        }
        let cancelled_at = now();
        entry.status = TaskStatus::Cancelled {
            cancelled_at,
            reason,
        };
        entry.updated_at = cancelled_at;
        Ok(entry.clone())
    }

    async fn retry(&self, id: &TaskId) -> SwarmResult<Task> {
        let mut entry = self
            .tasks
            .get_mut(id)
            .ok_or(SwarmError::TaskNotFound { id: *id })?;
        entry.retry().map_err(|status| SwarmError::InvalidTaskSpec {
            reason: format!(
                "only failed, cancelled, or timed out tasks can be retried from the task store; got {}",
                status.label()
            ),
        })?;
        Ok(entry.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tempfile::tempdir;
    use uuid::Uuid;

    use swarm_core::task::TaskSpec;

    fn task_with_known_id(id: &str) -> Task {
        let mut task = Task::new(TaskSpec::new("persisted-task", serde_json::json!({"x": 1})));
        task.id = TaskId::from_str(id).unwrap();
        task
    }

    #[tokio::test]
    async fn file_store_persists_recorded_tasks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("task-store.json");
        let store = FileTaskStore::new(&path);
        let task = task_with_known_id("00000000-0000-0000-0000-000000000001");

        store.record(task.clone()).await.unwrap();

        let second_store = FileTaskStore::new(&path);
        let tasks = second_store.list().await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
    }

    #[tokio::test]
    async fn cancel_marks_pending_task_as_cancelled() {
        let store = InMemoryTaskStore::new();
        let task = task_with_known_id("00000000-0000-0000-0000-000000000002");
        let id = task.id;
        store.record(task).await.unwrap();

        let cancelled = store.cancel(&id, Some("operator".into())).await.unwrap();

        assert!(matches!(cancelled.status, TaskStatus::Cancelled { .. }));
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().status.label(),
            "cancelled"
        );
    }

    #[tokio::test]
    async fn cancel_rejects_non_pending_tasks() {
        let store = InMemoryTaskStore::new();
        let mut task = task_with_known_id(&Uuid::nil().to_string());
        task.complete(serde_json::json!({"ok": true}));
        let id = task.id;
        store.record(task).await.unwrap();

        let error = store.cancel(&id, None).await.unwrap_err();
        assert!(matches!(error, SwarmError::InvalidTaskSpec { .. }));
    }

    #[tokio::test]
    async fn retry_requeues_failed_task() {
        let store = InMemoryTaskStore::new();
        let mut task = task_with_known_id("00000000-0000-0000-0000-000000000003");
        task.fail("transient");
        let id = task.id;
        store.record(task).await.unwrap();

        let retried = store.retry(&id).await.unwrap();

        assert_eq!(retried.status.label(), "pending");
        assert_eq!(
            store.get(&id).await.unwrap().unwrap().status.label(),
            "pending"
        );
    }

    #[tokio::test]
    async fn retry_rejects_completed_tasks() {
        let store = InMemoryTaskStore::new();
        let mut task = task_with_known_id("00000000-0000-0000-0000-000000000004");
        task.complete(serde_json::json!({"ok": true}));
        let id = task.id;
        store.record(task).await.unwrap();

        let error = store.retry(&id).await.unwrap_err();

        assert!(matches!(error, SwarmError::InvalidTaskSpec { .. }));
    }
}
