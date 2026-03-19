//! Task scheduler: matches pending tasks to available agents.
//!
//! The scheduler implements the capability-matching logic that decides which
//! agent should receive a given task. It considers:
//!
//! 1. **Capability matching**: the agent must possess all capabilities listed
//!    in `task.spec.required_capabilities`.
//! 2. **Availability**: the agent must be in the `Ready` state.
//! 3. **Role affinity**: when a task specifies a `role_hint`, agents with a
//!    matching `role_id` are preferred.
//! 4. **Load balancing**: when multiple agents qualify, the one with the fewest
//!    completed tasks (least-loaded) is preferred (simple bin-packing heuristic).

use swarm_core::{error::SwarmResult, identity::AgentId, task::Task};

use crate::registry::AgentRegistry;

/// The outcome of a scheduling attempt.
#[derive(Debug, Clone)]
pub enum SchedulingDecision {
    /// The task was successfully matched to an agent.
    Assigned {
        /// The task that was matched.
        task_id: swarm_core::identity::TaskId,
        /// The agent that was chosen.
        agent_id: AgentId,
    },
    /// No suitable agent was found. The task should remain queued.
    NoCapableAgent {
        /// The task that could not be scheduled.
        task_id: swarm_core::identity::TaskId,
    },
}

/// Stateless capability-based task scheduler.
///
/// The scheduler is intentionally stateless: all state lives in the
/// [`AgentRegistry`]. This makes it easy to swap the scheduling strategy
/// without changing any other component.
#[derive(Clone)]
pub struct Scheduler {
    registry: AgentRegistry,
}

impl Scheduler {
    /// Create a new scheduler backed by the given registry.
    pub fn new(registry: AgentRegistry) -> Self {
        Self { registry }
    }

    /// Attempt to find the best available agent for the given task.
    ///
    /// Returns [`SchedulingDecision::Assigned`] if a suitable agent was found,
    /// or [`SchedulingDecision::NoCapableAgent`] otherwise.
    ///
    /// When the task metadata contains a `role_hint` key, the scheduler
    /// prefers agents whose `role_id` matches. If no role-matching agent
    /// is available, any capable agent is used as a fallback.
    pub fn schedule(&self, task: &Task) -> SwarmResult<SchedulingDecision> {
        let required = &task.spec.required_capabilities;

        let role_hint: Option<String> = task
            .spec
            .metadata
            .get("role_hint")
            .map(|s| s.to_lowercase());

        // Collect candidates: agents that are available AND have the required capabilities.
        let mut candidates: Vec<_> = self
            .registry
            .all_agents()
            .into_iter()
            .filter(|record| {
                record.status.is_available()
                    && record.descriptor.capabilities.satisfies_all(required)
            })
            .collect();

        if candidates.is_empty() {
            tracing::debug!(
                task_id = %task.id,
                "No capable agent found for task"
            );
            return Ok(SchedulingDecision::NoCapableAgent { task_id: task.id });
        }

        // Role-affinity scoring: prefer agents whose role_id matches the hint.
        if let Some(ref hint) = role_hint {
            candidates.sort_by(|a, b| {
                let a_match = a
                    .descriptor
                    .role_id
                    .as_ref()
                    .map(|r| r.to_lowercase() == *hint)
                    .unwrap_or(false);
                let b_match = b
                    .descriptor
                    .role_id
                    .as_ref()
                    .map(|r| r.to_lowercase() == *hint)
                    .unwrap_or(false);
                // Sort role matches first (true > false → reversed).
                b_match
                    .cmp(&a_match)
                    .then_with(|| a.tasks_completed.cmp(&b.tasks_completed))
            });
        } else {
            // Least-loaded heuristic: prefer agents with fewer completed tasks.
            candidates.sort_by_key(|r| r.tasks_completed);
        }

        let chosen = &candidates[0];
        tracing::info!(
            task_id = %task.id,
            agent_id = %chosen.descriptor.id,
            agent_name = %chosen.descriptor.name,
            role_id = ?chosen.descriptor.role_id,
            "Task scheduled to agent"
        );

        Ok(SchedulingDecision::Assigned {
            task_id: task.id,
            agent_id: chosen.descriptor.id,
        })
    }

    /// Validate that the required capabilities in a task are satisfiable by
    /// *any* registered agent (not necessarily available right now).
    ///
    /// This is used as an admission check when a task is first submitted.
    pub fn is_satisfiable(&self, task: &Task) -> bool {
        let required = &task.spec.required_capabilities;
        if required.is_empty() {
            return true;
        }
        self.registry
            .all_agents()
            .iter()
            .any(|r| r.descriptor.capabilities.satisfies_all(required))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind, AgentStatus},
        capability::{Capability, CapabilitySet},
        task::{Task, TaskSpec},
    };

    fn make_registry_with_agent(name: &str, caps: CapabilitySet) -> (AgentRegistry, AgentId) {
        let registry = AgentRegistry::new();
        let desc = AgentDescriptor::new(name, AgentKind::Worker, caps);
        let id = registry.register(desc).unwrap();
        registry.update_status(&id, AgentStatus::Ready).unwrap();
        (registry, id)
    }

    #[test]
    fn schedules_to_capable_agent() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::new("text-generation"));
        let (registry, agent_id) = make_registry_with_agent("worker", caps);
        let scheduler = Scheduler::new(registry);

        let mut spec = TaskSpec::new("gen-task", serde_json::json!({}));
        spec.required_capabilities = {
            let mut c = CapabilitySet::new();
            c.add(Capability::new("text-generation"));
            c
        };
        let task = Task::new(spec);

        let decision = scheduler.schedule(&task).unwrap();
        assert!(
            matches!(decision, SchedulingDecision::Assigned { agent_id: id, .. } if id == agent_id)
        );
    }

    #[test]
    fn no_capable_agent_when_capability_missing() {
        let (registry, _) = make_registry_with_agent("worker", CapabilitySet::new());
        let scheduler = Scheduler::new(registry);

        let mut spec = TaskSpec::new("gen-task", serde_json::json!({}));
        spec.required_capabilities = {
            let mut c = CapabilitySet::new();
            c.add(Capability::new("image-analysis"));
            c
        };
        let task = Task::new(spec);

        let decision = scheduler.schedule(&task).unwrap();
        assert!(matches!(
            decision,
            SchedulingDecision::NoCapableAgent { .. }
        ));
    }

    #[test]
    fn unavailable_agent_not_scheduled() {
        let caps = CapabilitySet::new();
        let registry = AgentRegistry::new();
        let desc = AgentDescriptor::new("worker", AgentKind::Worker, caps);
        let id = registry.register(desc).unwrap();
        // Agent stays Inactive (not Ready)

        let scheduler = Scheduler::new(registry);
        let task = Task::new(TaskSpec::new("task", serde_json::json!({})));
        let decision = scheduler.schedule(&task).unwrap();
        assert!(matches!(
            decision,
            SchedulingDecision::NoCapableAgent { .. }
        ));
        let _ = id; // suppress unused warning
    }
}
