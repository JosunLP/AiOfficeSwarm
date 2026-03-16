//! Agent supervision tree management and fault escalation.
//!
//! The supervision manager tracks the hierarchical relationships between agents
//! and provides escalation logic when an agent fails. It is inspired by the
//! Erlang/OTP supervision model, adapted for the multi-tier executive/manager/
//! worker hierarchy.
//!
//! ## Escalation policy
//! When a worker agent fails, the failure is escalated to its manager (if one
//! exists). If the manager also fails, the failure escalates to the executive
//! level. The executive may decide to restart the agent, reassign its tasks,
//! or halt the affected workflow.

use dashmap::DashMap;
use std::sync::Arc;

use swarm_core::{
    agent::SupervisionTree,
    error::{SwarmError, SwarmResult},
    identity::AgentId,
};

/// Manages the supervision hierarchy for all registered agents.
///
/// The internal state is a map from `AgentId` to its [`SupervisionTree`] node.
#[derive(Clone, Default)]
pub struct SupervisionManager {
    nodes: Arc<DashMap<AgentId, SupervisionTree>>,
}

impl SupervisionManager {
    /// Create an empty supervision manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an agent as a root node (no supervisor).
    pub fn register_root(&self, agent_id: AgentId) {
        self.nodes
            .insert(agent_id, SupervisionTree::root(agent_id));
    }

    /// Register an agent under a supervisor.
    ///
    /// Both the subordinate and the supervisor must be registered.
    pub fn register_under(
        &self,
        agent_id: AgentId,
        supervisor_id: AgentId,
    ) -> SwarmResult<()> {
        if !self.nodes.contains_key(&supervisor_id) {
            return Err(SwarmError::AgentNotFound { id: supervisor_id });
        }
        // Add the new node as a subordinate to its supervisor.
        if let Some(mut supervisor_node) = self.nodes.get_mut(&supervisor_id) {
            supervisor_node.add_subordinate(agent_id);
        }
        // Register the new agent's own node.
        self.nodes.insert(
            agent_id,
            SupervisionTree::with_supervisor(agent_id, supervisor_id),
        );
        Ok(())
    }

    /// Remove an agent from the supervision tree.
    pub fn deregister(&self, agent_id: &AgentId) {
        if let Some((_, node)) = self.nodes.remove(agent_id) {
            // Remove this agent from its supervisor's subordinate list.
            if let Some(supervisor_id) = node.supervisor {
                if let Some(mut sup_node) = self.nodes.get_mut(&supervisor_id) {
                    sup_node.subordinates.retain(|id| id != agent_id);
                }
            }
        }
    }

    /// Return the supervisor of `agent_id`, if any.
    pub fn supervisor_of(&self, agent_id: &AgentId) -> Option<AgentId> {
        self.nodes
            .get(agent_id)
            .and_then(|n| n.supervisor)
    }

    /// Return the subordinates of `agent_id`.
    pub fn subordinates_of(&self, agent_id: &AgentId) -> Vec<AgentId> {
        self.nodes
            .get(agent_id)
            .map(|n| n.subordinates.clone())
            .unwrap_or_default()
    }

    /// Walk up the supervision tree from `agent_id` to find the first supervisor.
    ///
    /// Returns `None` if `agent_id` is already a root.
    pub fn escalation_target(&self, agent_id: &AgentId) -> Option<AgentId> {
        self.supervisor_of(agent_id)
    }

    /// Return the full ancestry chain (from agent up to root), not including
    /// the agent itself.
    pub fn ancestry(&self, agent_id: &AgentId) -> Vec<AgentId> {
        let mut chain = Vec::new();
        let mut current = *agent_id;
        while let Some(parent) = self.supervisor_of(&current) {
            chain.push(parent);
            current = parent;
        }
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_root_and_subordinate() {
        let mgr = SupervisionManager::new();
        let exec = AgentId::new();
        let worker = AgentId::new();

        mgr.register_root(exec);
        mgr.register_under(worker, exec).unwrap();

        assert_eq!(mgr.supervisor_of(&worker), Some(exec));
        assert_eq!(mgr.subordinates_of(&exec), vec![worker]);
    }

    #[test]
    fn register_under_unknown_supervisor_fails() {
        let mgr = SupervisionManager::new();
        let unknown = AgentId::new();
        let worker = AgentId::new();

        let result = mgr.register_under(worker, unknown);
        assert!(result.is_err());
    }

    #[test]
    fn ancestry_chain() {
        let mgr = SupervisionManager::new();
        let exec = AgentId::new();
        let manager = AgentId::new();
        let worker = AgentId::new();

        mgr.register_root(exec);
        mgr.register_under(manager, exec).unwrap();
        mgr.register_under(worker, manager).unwrap();

        let chain = mgr.ancestry(&worker);
        assert_eq!(chain, vec![manager, exec]);
    }

    #[test]
    fn deregister_removes_from_supervisor() {
        let mgr = SupervisionManager::new();
        let exec = AgentId::new();
        let worker = AgentId::new();

        mgr.register_root(exec);
        mgr.register_under(worker, exec).unwrap();
        mgr.deregister(&worker);

        assert!(mgr.subordinates_of(&exec).is_empty());
    }
}
