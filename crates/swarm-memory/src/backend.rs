//! The core backend trait for memory storage.

use async_trait::async_trait;

use swarm_core::error::SwarmResult;

use crate::entry::{MemoryEntry, MemoryId};
use crate::query::MemoryQuery;
use crate::retention::RetentionPolicy;

/// The primary trait for memory storage backends.
///
/// Implement this trait to provide a memory backend (in-memory, database,
/// vector store, etc.). The framework never accesses storage directly — all
/// memory I/O goes through this trait.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Store a memory entry and return its ID.
    async fn store(&self, entry: MemoryEntry) -> SwarmResult<MemoryId>;

    /// Retrieve memory entries matching the given query.
    async fn retrieve(&self, query: &MemoryQuery) -> SwarmResult<Vec<MemoryEntry>>;

    /// Retrieve a single entry by ID.
    async fn get(&self, id: &MemoryId) -> SwarmResult<Option<MemoryEntry>>;

    /// Delete a memory entry by ID.
    async fn delete(&self, id: &MemoryId) -> SwarmResult<()>;

    /// Apply a retention policy: expire/remove entries that exceed the policy
    /// limits. Returns the number of entries removed.
    async fn apply_retention(&self, policy: &RetentionPolicy) -> SwarmResult<u64>;

    /// Redact specific fields from a memory entry (replace with `[REDACTED]`).
    /// Returns `Ok(())` if the entry was found and redacted.
    async fn redact(&self, id: &MemoryId, fields: &[String]) -> SwarmResult<()>;

    /// Perform a health check on the backend.
    async fn health_check(&self) -> SwarmResult<()>;
}
