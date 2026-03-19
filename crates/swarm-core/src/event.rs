//! Event model for the swarm event bus.
//!
//! The framework is event-driven: significant state changes in the orchestrator
//! emit events that are consumed by monitors, policies, plugins, and audit
//! loggers. Events are the primary integration surface for observability
//! and reactive automation.
//!
//! Events are delivered via an in-process broadcast channel (Tokio's
//! `broadcast` channel is the default backend in the orchestrator crate).
//! Future versions may plug in external message brokers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::{AgentId, PluginId, TaskId};

/// The discriminant describing what happened.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    // ── Agent lifecycle events ─────────────────────────────────────────────
    /// An agent was registered with the orchestrator.
    AgentRegistered { agent_id: AgentId, name: String },
    /// An agent's status changed.
    AgentStatusChanged {
        agent_id: AgentId,
        previous: String,
        current: String,
    },
    /// An agent was deregistered.
    AgentDeregistered { agent_id: AgentId },

    // ── Task lifecycle events ──────────────────────────────────────────────
    /// A task was submitted to the orchestrator.
    TaskSubmitted { task_id: TaskId, name: String },
    /// A task was assigned to an agent.
    TaskScheduled { task_id: TaskId, agent_id: AgentId },
    /// A task began execution.
    TaskStarted { task_id: TaskId, agent_id: AgentId },
    /// A task completed successfully.
    TaskCompleted { task_id: TaskId },
    /// A task failed.
    TaskFailed { task_id: TaskId, reason: String },
    /// A task was cancelled.
    TaskCancelled { task_id: TaskId },
    /// A task timed out.
    TaskTimedOut { task_id: TaskId },

    // ── Policy events ──────────────────────────────────────────────────────
    /// A policy was evaluated.
    PolicyEvaluated {
        action: String,
        subject: String,
        decision: String,
    },

    // ── Plugin events ──────────────────────────────────────────────────────
    /// A plugin was loaded and initialized.
    PluginLoaded { plugin_id: PluginId, name: String },
    /// A plugin was unloaded.
    PluginUnloaded { plugin_id: PluginId, name: String },
    /// A plugin emitted a custom domain event.
    PluginEvent {
        plugin_id: PluginId,
        payload: serde_json::Value,
    },

    // ── System events ──────────────────────────────────────────────────────
    /// The orchestrator started.
    OrchestratorStarted,
    /// The orchestrator is shutting down.
    OrchestratorShuttingDown,
}

/// A domain event that has been observed in the system.
///
/// Every event is wrapped in an `EventEnvelope` which adds routing and
/// audit metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique event identifier (for deduplication and idempotency).
    pub id: Uuid,
    /// When the event was produced.
    pub timestamp: DateTime<Utc>,
    /// The event payload.
    pub kind: EventKind,
    /// Optional correlation ID linking related events (e.g., a task pipeline).
    pub correlation_id: Option<Uuid>,
    /// Optional source label (useful for tracing in distributed deployments).
    pub source: Option<String>,
}

impl EventEnvelope {
    /// Wrap an [`EventKind`] in an envelope with a new unique ID and timestamp.
    pub fn new(kind: EventKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            kind,
            correlation_id: None,
            source: None,
        }
    }

    /// Attach a correlation ID to this envelope.
    pub fn with_correlation(mut self, id: Uuid) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Attach a source label to this envelope.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Type alias kept for ergonomic use at call sites.
pub type Event = EventEnvelope;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_envelope_has_unique_ids() {
        let a = EventEnvelope::new(EventKind::OrchestratorStarted);
        let b = EventEnvelope::new(EventKind::OrchestratorStarted);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn event_envelope_with_correlation() {
        let corr = Uuid::new_v4();
        let ev = EventEnvelope::new(EventKind::OrchestratorStarted).with_correlation(corr);
        assert_eq!(ev.correlation_id, Some(corr));
    }
}
