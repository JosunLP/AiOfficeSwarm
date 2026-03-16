//! Agent registry: tracks all registered agents and their runtime state.
//!
//! The registry is the authoritative source of truth about which agents exist
//! and what their current status is. It is analogous to the Kubernetes API
//! server's object store for Pod resources.

use dashmap::DashMap;
use std::sync::Arc;

use swarm_core::{
    agent::{AgentDescriptor, AgentStatus},
    error::{SwarmError, SwarmResult},
    identity::AgentId,
    types::Timestamp,
};

/// A record combining an agent's static descriptor with its live runtime state.
#[derive(Debug, Clone)]
pub struct AgentRecord {
    /// The static descriptor registered by the agent implementation.
    pub descriptor: AgentDescriptor,
    /// Current operational status.
    pub status: AgentStatus,
    /// Timestamp of the last status update.
    pub last_seen_at: Timestamp,
    /// Total number of tasks completed by this agent since registration.
    pub tasks_completed: u64,
    /// Total number of tasks failed by this agent since registration.
    pub tasks_failed: u64,
}

impl AgentRecord {
    /// Create a new record for a freshly registered agent.
    pub fn new(descriptor: AgentDescriptor) -> Self {
        Self {
            last_seen_at: swarm_core::types::now(),
            descriptor,
            status: AgentStatus::Inactive,
            tasks_completed: 0,
            tasks_failed: 0,
        }
    }

    /// Update the status and refresh the `last_seen_at` timestamp.
    pub fn update_status(&mut self, status: AgentStatus) {
        self.status = status;
        self.last_seen_at = swarm_core::types::now();
    }
}

/// Thread-safe, in-memory agent registry.
///
/// Uses a `DashMap` for lock-free concurrent reads, which is important because
/// the scheduler reads agent records frequently during task dispatch.
#[derive(Clone, Default)]
pub struct AgentRegistry {
    agents: Arc<DashMap<AgentId, AgentRecord>>,
}

impl AgentRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            agents: Arc::new(DashMap::new()),
        }
    }

    /// Register an agent. Returns an error if an agent with the same ID already
    /// exists.
    pub fn register(&self, descriptor: AgentDescriptor) -> SwarmResult<AgentId> {
        let id = descriptor.id;
        if self.agents.contains_key(&id) {
            return Err(SwarmError::Internal {
                reason: format!("agent {} is already registered", id),
            });
        }
        let record = AgentRecord::new(descriptor);
        self.agents.insert(id, record);
        tracing::info!(agent_id = %id, "Agent registered");
        Ok(id)
    }

    /// Deregister an agent by ID.
    pub fn deregister(&self, id: &AgentId) -> SwarmResult<AgentRecord> {
        self.agents
            .remove(id)
            .map(|(_, record)| record)
            .ok_or_else(|| SwarmError::AgentNotFound { id: *id })
    }

    /// Update an agent's status. Emits a tracing event.
    pub fn update_status(&self, id: &AgentId, status: AgentStatus) -> SwarmResult<()> {
        let mut record = self
            .agents
            .get_mut(id)
            .ok_or_else(|| SwarmError::AgentNotFound { id: *id })?;
        let prev_label = record.status.label().to_owned();
        let new_label = status.label().to_owned();
        record.update_status(status);
        tracing::debug!(
            agent_id = %id,
            previous = prev_label,
            current = new_label,
            "Agent status changed"
        );
        Ok(())
    }

    /// Retrieve a snapshot of an agent record.
    pub fn get(&self, id: &AgentId) -> SwarmResult<AgentRecord> {
        self.agents
            .get(id)
            .map(|r| r.clone())
            .ok_or_else(|| SwarmError::AgentNotFound { id: *id })
    }

    /// Return IDs of all agents currently in the `Ready` state.
    pub fn available_agents(&self) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|r| r.status.is_available())
            .map(|r| r.descriptor.id)
            .collect()
    }

    /// Return a snapshot of all agent records.
    pub fn all_agents(&self) -> Vec<AgentRecord> {
        self.agents.iter().map(|r| r.clone()).collect()
    }

    /// Return the number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Returns `true` if no agents are registered.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Increment the completed-task counter for the given agent.
    pub fn record_task_completed(&self, id: &AgentId) {
        if let Some(mut r) = self.agents.get_mut(id) {
            r.tasks_completed += 1;
        }
    }

    /// Increment the failed-task counter for the given agent.
    pub fn record_task_failed(&self, id: &AgentId) {
        if let Some(mut r) = self.agents.get_mut(id) {
            r.tasks_failed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind},
        capability::CapabilitySet,
    };

    fn make_descriptor(name: &str) -> AgentDescriptor {
        AgentDescriptor::new(name, AgentKind::Worker, CapabilitySet::new())
    }

    #[test]
    fn register_and_retrieve() {
        let registry = AgentRegistry::new();
        let desc = make_descriptor("worker-1");
        let id = registry.register(desc.clone()).expect("should register");
        let record = registry.get(&id).expect("should find");
        assert_eq!(record.descriptor.name, "worker-1");
    }

    #[test]
    fn register_duplicate_fails() {
        let registry = AgentRegistry::new();
        let desc = make_descriptor("worker-1");
        let id = registry.register(desc.clone()).unwrap();
        // Re-register same id
        let desc2 = AgentDescriptor {
            id,
            ..make_descriptor("worker-1")
        };
        assert!(registry.register(desc2).is_err());
    }

    #[test]
    fn update_status_and_available_agents() {
        let registry = AgentRegistry::new();
        let desc = make_descriptor("worker-1");
        let id = registry.register(desc).unwrap();

        // Initially inactive, not available
        assert!(registry.available_agents().is_empty());

        // Mark as ready
        registry.update_status(&id, AgentStatus::Ready).unwrap();
        assert_eq!(registry.available_agents().len(), 1);
    }

    #[test]
    fn deregister_removes_agent() {
        let registry = AgentRegistry::new();
        let desc = make_descriptor("worker-1");
        let id = registry.register(desc).unwrap();
        registry.deregister(&id).expect("should deregister");
        assert!(registry.get(&id).is_err());
    }
}
