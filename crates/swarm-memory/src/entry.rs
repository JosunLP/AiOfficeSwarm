//! Memory entry types: the data stored in the memory subsystem.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unique identifier for a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(Uuid);

impl MemoryId {
    /// Create a new, random memory identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from a known UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Return the underlying UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MemoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The scope of a memory entry — determines lifetime and access boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryScope {
    /// Scoped to a single interaction/session.
    Session {
        /// The session identifier.
        session_id: String,
    },
    /// Scoped to a single task execution.
    Task {
        /// The task identifier.
        task_id: String,
    },
    /// Scoped to a single agent's lifetime.
    Agent {
        /// The agent identifier.
        agent_id: String,
    },
    /// Shared across a team of agents.
    Team {
        /// The team identifier.
        team_id: String,
    },
    /// Shared across a tenant/organization.
    Tenant {
        /// The tenant identifier.
        tenant_id: String,
    },
    /// Persistent long-term memory (survives restarts).
    LongTerm {
        /// The owner identifier (agent, team, or tenant).
        owner_id: String,
    },
}

impl MemoryScope {
    /// Returns a compact label for logging.
    pub fn label(&self) -> &'static str {
        match self {
            MemoryScope::Session { .. } => "session",
            MemoryScope::Task { .. } => "task",
            MemoryScope::Agent { .. } => "agent",
            MemoryScope::Team { .. } => "team",
            MemoryScope::Tenant { .. } => "tenant",
            MemoryScope::LongTerm { .. } => "long_term",
        }
    }
}

/// The type of memory content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    /// Key-value or tabular structured data.
    Structured,
    /// Vector-embeddable semantic content (for similarity search).
    Semantic,
    /// Timestamped event records (past actions, interactions).
    Episodic,
    /// Step sequences or reusable plans.
    Procedural,
    /// Reference/pointer to external knowledge (URI, document ID).
    KnowledgeRef,
    /// Compressed summary of other memory entries.
    Summary,
    /// External linked record (CRM, ticket, database row).
    ExternalLink,
}

impl MemoryType {
    /// Returns a compact label for logging.
    pub fn label(&self) -> &'static str {
        match self {
            MemoryType::Structured => "structured",
            MemoryType::Semantic => "semantic",
            MemoryType::Episodic => "episodic",
            MemoryType::Procedural => "procedural",
            MemoryType::KnowledgeRef => "knowledge_ref",
            MemoryType::Summary => "summary",
            MemoryType::ExternalLink => "external_link",
        }
    }
}

/// Sensitivity level for memory content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SensitivityLevel {
    /// Public or non-sensitive information.
    Public = 0,
    /// Internal/organization-only information.
    #[default]
    Internal = 1,
    /// Confidential — restricted access.
    Confidential = 2,
    /// Highly restricted — PII, secrets, etc.
    Restricted = 3,
}

/// A single memory entry stored in the memory subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique entry identifier.
    pub id: MemoryId,
    /// The scope this entry belongs to.
    pub scope: MemoryScope,
    /// The type of memory content.
    pub memory_type: MemoryType,
    /// The content payload (free-form JSON).
    pub content: serde_json::Value,
    /// Optional embedding vector (for semantic memory).
    pub embedding: Option<Vec<f64>>,
    /// Sensitivity level for access control and audit.
    pub sensitivity: SensitivityLevel,
    /// Arbitrary tags for filtering and categorization.
    pub tags: Vec<String>,
    /// Arbitrary metadata.
    pub metadata: HashMap<String, String>,
    /// When this entry was created.
    pub created_at: DateTime<Utc>,
    /// When this entry was last accessed.
    pub last_accessed_at: Option<DateTime<Utc>>,
    /// When this entry expires (if set).
    pub expires_at: Option<DateTime<Utc>>,
}

impl MemoryEntry {
    /// Create a new memory entry with minimal fields.
    pub fn new(scope: MemoryScope, memory_type: MemoryType, content: serde_json::Value) -> Self {
        Self {
            id: MemoryId::new(),
            scope,
            memory_type,
            content,
            embedding: None,
            sensitivity: SensitivityLevel::default(),
            tags: Vec::new(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
            last_accessed_at: None,
            expires_at: None,
        }
    }

    /// Returns `true` if this entry has expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => Utc::now() > exp,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_entry_creation() {
        let entry = MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: "agent-1".into(),
            },
            MemoryType::Structured,
            serde_json::json!({"key": "value"}),
        );
        assert_eq!(entry.scope.label(), "agent");
        assert_eq!(entry.memory_type.label(), "structured");
        assert!(!entry.is_expired());
    }

    #[test]
    fn sensitivity_ordering() {
        assert!(SensitivityLevel::Public < SensitivityLevel::Internal);
        assert!(SensitivityLevel::Internal < SensitivityLevel::Confidential);
        assert!(SensitivityLevel::Confidential < SensitivityLevel::Restricted);
    }
}
