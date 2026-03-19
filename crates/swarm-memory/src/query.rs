//! Memory query types for retrieval operations.

use serde::{Deserialize, Serialize};

use crate::entry::{MemoryScope, MemoryType, SensitivityLevel};

/// A query for retrieving memory entries.
///
/// All criteria are optional — unset fields are not filtered on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryQuery {
    /// Filter by memory scope.
    pub scope: Option<MemoryScope>,
    /// Filter by memory type.
    pub memory_type: Option<MemoryType>,
    /// Filter by tags (entries must have ALL of these tags).
    pub tags: Vec<String>,
    /// Full-text search query (backend-dependent behavior).
    pub text_query: Option<String>,
    /// Semantic similarity search vector.
    pub embedding_query: Option<Vec<f64>>,
    /// Maximum number of results to return.
    pub limit: Option<usize>,
    /// Maximum sensitivity level the caller is allowed to see.
    pub max_sensitivity: Option<SensitivityLevel>,
    /// Whether to include expired entries.
    pub include_expired: bool,
}

impl MemoryQuery {
    /// Create an empty query (matches everything).
    pub fn all() -> Self {
        Self::default()
    }

    /// Filter by scope.
    pub fn with_scope(mut self, scope: MemoryScope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Filter by memory type.
    pub fn with_type(mut self, memory_type: MemoryType) -> Self {
        self.memory_type = Some(memory_type);
        self
    }

    /// Filter by tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Set a result limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set maximum sensitivity level.
    pub fn with_max_sensitivity(mut self, level: SensitivityLevel) -> Self {
        self.max_sensitivity = Some(level);
        self
    }

    /// Perform a semantic similarity query.
    pub fn with_embedding(mut self, embedding: Vec<f64>) -> Self {
        self.embedding_query = Some(embedding);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_builder() {
        let q = MemoryQuery::all()
            .with_scope(MemoryScope::Agent {
                agent_id: "a1".into(),
            })
            .with_type(MemoryType::Semantic)
            .with_limit(10);
        assert_eq!(q.limit, Some(10));
        assert!(q.scope.is_some());
        assert!(q.memory_type.is_some());
    }
}
