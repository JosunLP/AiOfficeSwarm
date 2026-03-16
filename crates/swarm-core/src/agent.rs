//! Agent domain model and traits.
//!
//! An [`Agent`] is the central abstraction in the swarm. Agents are typed
//! workers that execute tasks according to their capabilities. The framework
//! supports a three-tier hierarchy:
//!
//! | Tier | Kind | Responsibility |
//! |------|------|----------------|
//! | 1 | [`AgentKind::Executive`] | Strategic direction, cross-domain arbitration |
//! | 2 | [`AgentKind::Manager`] | Coordination within a domain, workload distribution |
//! | 3 | [`AgentKind::Worker`] | Execution of concrete tasks |
//!
//! The [`Agent`] trait is the primary extension point: third-party crates
//! implement this trait to provide custom agent behaviours.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::error::SwarmResult;
use crate::identity::AgentId;
use crate::task::Task;
use crate::types::{Metadata, ResourceLimits};

/// Defines the hierarchical role of an agent within the swarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentKind {
    /// Strategic coordinator. May delegate to managers or workers.
    Executive,
    /// Tactical coordinator for a domain. May delegate to workers.
    Manager,
    /// Concrete task executor. Performs work directly.
    Worker,
}

impl AgentKind {
    /// Returns a short label suitable for metrics and logging.
    pub fn label(&self) -> &'static str {
        match self {
            AgentKind::Executive => "executive",
            AgentKind::Manager => "manager",
            AgentKind::Worker => "worker",
        }
    }
}

/// The current operational status of an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// Agent has been registered but has not yet started.
    Inactive,
    /// Agent is running and ready to accept tasks.
    Ready,
    /// Agent is currently busy executing a task.
    Busy {
        /// The task currently being executed.
        current_task: crate::identity::TaskId,
    },
    /// Agent is temporarily unable to accept tasks.
    Paused {
        /// Human-readable reason for the pause.
        reason: String,
    },
    /// Agent has encountered an unrecoverable error and is no longer usable.
    Failed {
        /// Description of the failure.
        reason: String,
        /// When the failure was recorded.
        failed_at: DateTime<Utc>,
    },
    /// Agent has been gracefully shut down.
    Stopped,
}

impl AgentStatus {
    /// Returns `true` if the agent can accept new tasks.
    pub fn is_available(&self) -> bool {
        matches!(self, AgentStatus::Ready)
    }

    /// Returns a short label for metrics and logging.
    pub fn label(&self) -> &'static str {
        match self {
            AgentStatus::Inactive => "inactive",
            AgentStatus::Ready => "ready",
            AgentStatus::Busy { .. } => "busy",
            AgentStatus::Paused { .. } => "paused",
            AgentStatus::Failed { .. } => "failed",
            AgentStatus::Stopped => "stopped",
        }
    }
}

/// A static registration record describing an agent to the orchestrator.
///
/// This is analogous to a Kubernetes `PodSpec` — it is the *desired* configuration
/// of an agent, separate from its runtime state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDescriptor {
    /// Unique agent identifier.
    pub id: AgentId,
    /// Human-readable name for display purposes.
    pub name: String,
    /// The hierarchical role of this agent.
    pub kind: AgentKind,
    /// Capabilities this agent can provide.
    pub capabilities: CapabilitySet,
    /// Resource consumption limits.
    pub resource_limits: ResourceLimits,
    /// Arbitrary key-value labels.
    pub metadata: Metadata,
    /// When this agent descriptor was created.
    pub registered_at: DateTime<Utc>,
}

impl AgentDescriptor {
    /// Create a new descriptor with sensible defaults.
    pub fn new(
        name: impl Into<String>,
        kind: AgentKind,
        capabilities: CapabilitySet,
    ) -> Self {
        Self {
            id: AgentId::new(),
            name: name.into(),
            kind,
            capabilities,
            resource_limits: ResourceLimits::default(),
            metadata: Metadata::new(),
            registered_at: Utc::now(),
        }
    }
}

/// The primary extension point for agent implementations.
///
/// Implement this trait to create a custom agent that can be registered with
/// the orchestrator.
///
/// ## Example
/// ```rust,no_run
/// use swarm_core::agent::{Agent, AgentDescriptor};
/// use swarm_core::task::Task;
/// use swarm_core::error::SwarmResult;
/// use async_trait::async_trait;
///
/// struct EchoAgent {
///     descriptor: AgentDescriptor,
/// }
///
/// #[async_trait]
/// impl Agent for EchoAgent {
///     fn descriptor(&self) -> &AgentDescriptor {
///         &self.descriptor
///     }
///
///     async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
///         Ok(task.spec.input.clone())
///     }
///
///     async fn health_check(&self) -> SwarmResult<()> {
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait Agent: Send + Sync {
    /// Returns the static descriptor for this agent.
    fn descriptor(&self) -> &AgentDescriptor;

    /// Execute a task and return its output.
    ///
    /// This is the core method of the [`Agent`] trait. Implementations should
    /// be async and respect the timeout in `task.spec.timeout` if set.
    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value>;

    /// Perform a health check and return `Ok(())` if the agent is healthy.
    async fn health_check(&self) -> SwarmResult<()>;

    /// Called once before the agent starts accepting tasks. Use for
    /// initialization, connection setup, etc.
    ///
    /// The default implementation does nothing.
    async fn on_start(&mut self) -> SwarmResult<()> {
        Ok(())
    }

    /// Called when the agent is being shut down gracefully. Use for cleanup.
    ///
    /// The default implementation does nothing.
    async fn on_stop(&mut self) -> SwarmResult<()> {
        Ok(())
    }
}

/// A node in the agent supervision tree.
///
/// Each node references a supervisor (parent) agent and zero or more
/// subordinate (child) agents. The supervision tree determines escalation
/// paths when an agent fails or a task cannot be completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisionTree {
    /// This node's agent.
    pub agent_id: AgentId,
    /// The agent responsible for supervising this one, if any.
    pub supervisor: Option<AgentId>,
    /// Agents supervised by this agent.
    pub subordinates: Vec<AgentId>,
}

impl SupervisionTree {
    /// Create a root node (no supervisor).
    pub fn root(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            supervisor: None,
            subordinates: Vec::new(),
        }
    }

    /// Create a node with the given supervisor.
    pub fn with_supervisor(agent_id: AgentId, supervisor: AgentId) -> Self {
        Self {
            agent_id,
            supervisor: Some(supervisor),
            subordinates: Vec::new(),
        }
    }

    /// Register a new subordinate under this node.
    pub fn add_subordinate(&mut self, agent_id: AgentId) {
        if !self.subordinates.contains(&agent_id) {
            self.subordinates.push(agent_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_availability() {
        assert!(AgentStatus::Ready.is_available());
        assert!(!AgentStatus::Busy { current_task: crate::identity::TaskId::new() }.is_available());
        assert!(!AgentStatus::Stopped.is_available());
    }

    #[test]
    fn supervision_tree_root_has_no_supervisor() {
        let id = AgentId::new();
        let tree = SupervisionTree::root(id);
        assert!(tree.supervisor.is_none());
        assert!(tree.subordinates.is_empty());
    }

    #[test]
    fn supervision_tree_add_subordinate_no_duplicates() {
        let supervisor_id = AgentId::new();
        let subordinate_id = AgentId::new();
        let mut tree = SupervisionTree::root(supervisor_id);
        tree.add_subordinate(subordinate_id);
        tree.add_subordinate(subordinate_id); // duplicate should be ignored
        assert_eq!(tree.subordinates.len(), 1);
    }
}
