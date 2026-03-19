//! In-memory backend implementation for development and testing.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;

use swarm_core::error::{SwarmError, SwarmResult};

use crate::backend::MemoryBackend;
use crate::entry::{MemoryEntry, MemoryId};
use crate::query::MemoryQuery;
use crate::retention::RetentionPolicy;

/// A simple in-memory implementation of [`MemoryBackend`].
///
/// Suitable for development, testing, and single-process deployments.
/// Not suitable for production use where persistence is required.
pub struct InMemoryBackend {
    entries: Arc<DashMap<MemoryId, MemoryEntry>>,
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryBackend {
    /// Create an empty in-memory backend.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl MemoryBackend for InMemoryBackend {
    async fn store(&self, entry: MemoryEntry) -> SwarmResult<MemoryId> {
        let id = entry.id;
        self.entries.insert(id, entry);
        Ok(id)
    }

    async fn retrieve(&self, query: &MemoryQuery) -> SwarmResult<Vec<MemoryEntry>> {
        let mut results: Vec<MemoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                let entry = e.value();

                // Skip expired unless requested.
                if !query.include_expired && entry.is_expired() {
                    return false;
                }

                // Filter by scope.
                if let Some(ref scope) = query.scope {
                    if entry.scope != *scope {
                        return false;
                    }
                }

                // Filter by type.
                if let Some(ref mt) = query.memory_type {
                    if entry.memory_type != *mt {
                        return false;
                    }
                }

                // Filter by tags (all must match).
                if !query.tags.is_empty() && !query.tags.iter().all(|t| entry.tags.contains(t)) {
                    return false;
                }

                // Filter by sensitivity.
                if let Some(max_sens) = query.max_sensitivity {
                    if entry.sensitivity > max_sens {
                        return false;
                    }
                }

                true
            })
            .map(|e| e.value().clone())
            .collect();

        // Sort by created_at descending (most recent first).
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply limit.
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn get(&self, id: &MemoryId) -> SwarmResult<Option<MemoryEntry>> {
        Ok(self.entries.get(id).map(|e| e.value().clone()))
    }

    async fn delete(&self, id: &MemoryId) -> SwarmResult<()> {
        self.entries
            .remove(id)
            .ok_or_else(|| SwarmError::Internal {
                reason: format!("Memory entry {} not found", id),
            })?;
        Ok(())
    }

    async fn apply_retention(&self, policy: &RetentionPolicy) -> SwarmResult<u64> {
        let now = Utc::now();
        let mut removed = 0u64;

        // Remove entries exceeding max_age.
        if let Some(max_age) = policy.max_age {
            let cutoff = now
                - chrono::Duration::from_std(max_age).map_err(|e| SwarmError::Internal {
                    reason: e.to_string(),
                })?;

            let to_remove: Vec<MemoryId> = self
                .entries
                .iter()
                .filter(|e| {
                    let entry = e.value();
                    if let Some(ref scope) = policy.scope {
                        if entry.scope != *scope {
                            return false;
                        }
                    }
                    entry.created_at < cutoff
                })
                .map(|e| *e.key())
                .collect();

            for id in to_remove {
                self.entries.remove(&id);
                removed += 1;
            }
        }

        // Enforce max_entries by removing oldest.
        if let Some(max_entries) = policy.max_entries {
            let mut scoped: Vec<(MemoryId, chrono::DateTime<Utc>)> = self
                .entries
                .iter()
                .filter(|e| {
                    if let Some(ref scope) = policy.scope {
                        e.value().scope == *scope
                    } else {
                        true
                    }
                })
                .map(|e| (*e.key(), e.value().created_at))
                .collect();

            if scoped.len() as u64 > max_entries {
                // Sort oldest first.
                scoped.sort_by_key(|(_, ts)| *ts);
                let excess = scoped.len() as u64 - max_entries;
                for (id, _) in scoped.into_iter().take(excess as usize) {
                    self.entries.remove(&id);
                    removed += 1;
                }
            }
        }

        Ok(removed)
    }

    async fn redact(&self, id: &MemoryId, fields: &[String]) -> SwarmResult<()> {
        let mut entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| SwarmError::Internal {
                reason: format!("Memory entry {} not found", id),
            })?;

        if let serde_json::Value::Object(ref mut map) = entry.content {
            for field in fields {
                if map.contains_key(field) {
                    map.insert(
                        field.clone(),
                        serde_json::Value::String("[REDACTED]".into()),
                    );
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{MemoryScope, MemoryType};
    use crate::query::MemoryQuery;

    #[tokio::test]
    async fn store_and_retrieve() {
        let backend = InMemoryBackend::new();
        let scope = MemoryScope::Agent {
            agent_id: "a1".into(),
        };
        let entry = MemoryEntry::new(
            scope.clone(),
            MemoryType::Structured,
            serde_json::json!({"key": "value"}),
        );
        let id = backend.store(entry).await.unwrap();

        let results = backend
            .retrieve(&MemoryQuery::all().with_scope(scope))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, id);
    }

    #[tokio::test]
    async fn delete_entry() {
        let backend = InMemoryBackend::new();
        let entry = MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: "a1".into(),
            },
            MemoryType::Episodic,
            serde_json::json!({}),
        );
        let id = backend.store(entry).await.unwrap();
        backend.delete(&id).await.unwrap();
        assert!(backend.get(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn redact_fields() {
        let backend = InMemoryBackend::new();
        let entry = MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: "a1".into(),
            },
            MemoryType::Structured,
            serde_json::json!({"name": "John", "email": "john@example.com", "role": "admin"}),
        );
        let id = backend.store(entry).await.unwrap();
        backend.redact(&id, &["email".into()]).await.unwrap();

        let retrieved = backend.get(&id).await.unwrap().unwrap();
        assert_eq!(retrieved.content["email"], "[REDACTED]");
        assert_eq!(retrieved.content["name"], "John");
    }
}
