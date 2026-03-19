//! Learning events — the observations that feed into learning strategies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A learning-relevant event observed during agent operation.
///
/// Learning strategies subscribe to these events and decide whether to
/// produce learning outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEvent {
    /// What kind of observation this is.
    pub kind: LearningEventKind,
    /// The agent that produced or is associated with this event.
    pub agent_id: String,
    /// The task context (if applicable).
    pub task_id: Option<String>,
    /// The tenant this event belongs to.
    pub tenant_id: Option<String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Arbitrary structured data about the event.
    pub payload: serde_json::Value,
}

/// Discriminant for learning event types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LearningEventKind {
    /// A task was completed successfully.
    TaskCompleted,
    /// A task failed.
    TaskFailed,
    /// Explicit feedback was provided (human or automated).
    FeedbackReceived,
    /// An agent selected a plan or strategy.
    PlanSelected,
    /// An agent escalated to its supervisor.
    Escalation,
    /// A provider was selected for a request.
    ProviderSelected,
    /// A custom learning-relevant event.
    Custom(String),
}

impl LearningEvent {
    /// Create a task-completed event.
    pub fn task_completed(
        agent_id: impl Into<String>,
        task_id: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            kind: LearningEventKind::TaskCompleted,
            agent_id: agent_id.into(),
            task_id: Some(task_id.into()),
            tenant_id: None,
            timestamp: Utc::now(),
            payload,
        }
    }

    /// Create a feedback-received event.
    pub fn feedback(agent_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: LearningEventKind::FeedbackReceived,
            agent_id: agent_id.into(),
            task_id: None,
            tenant_id: None,
            timestamp: Utc::now(),
            payload,
        }
    }
}
