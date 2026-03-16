//! The top-level orchestrator: coordinates agent registry, scheduler,
//! supervision, and event bus.
//!
//! The [`Orchestrator`] is the single control-plane component that glues
//! everything together. Users interact with it via an [`OrchestratorHandle`]
//! which exposes an async API.

use std::sync::Arc;
use tokio::sync::broadcast;

use swarm_core::{
    agent::{AgentDescriptor, AgentStatus},
    error::{SwarmError, SwarmResult},
    event::{Event, EventEnvelope, EventKind},
    identity::{AgentId, TaskId},
    task::{Task, TaskSpec, TaskStatus},
};

use crate::{
    registry::AgentRegistry,
    scheduler::{SchedulingDecision, Scheduler},
    supervision::SupervisionManager,
    task_queue::TaskQueue,
};

/// Configuration for the orchestrator.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Capacity of the internal event broadcast channel.
    pub event_channel_capacity: usize,
    /// How many dispatch iterations to run per scheduling tick.
    pub max_dispatch_per_tick: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            event_channel_capacity: 1024,
            max_dispatch_per_tick: 16,
        }
    }
}

/// Internal shared state of the orchestrator.
struct OrchestratorState {
    registry: AgentRegistry,
    task_queue: TaskQueue,
    /// Map from TaskId → Task for in-flight and recently completed tasks.
    tasks: dashmap::DashMap<TaskId, Task>,
    supervision: SupervisionManager,
    scheduler: Scheduler,
    event_tx: broadcast::Sender<Event>,
}

impl OrchestratorState {
    fn new(config: &OrchestratorConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.event_channel_capacity);
        let registry = AgentRegistry::new();
        let scheduler = Scheduler::new(registry.clone());
        Self {
            registry,
            task_queue: TaskQueue::new(),
            tasks: dashmap::DashMap::new(),
            supervision: SupervisionManager::new(),
            scheduler,
            event_tx,
        }
    }

    fn emit(&self, kind: EventKind) {
        let ev = EventEnvelope::new(kind);
        // It's OK if there are no subscribers.
        let _ = self.event_tx.send(ev);
    }
}

/// The primary orchestrator struct.
///
/// Create one via [`Orchestrator::new`] and then call [`Orchestrator::handle`]
/// to obtain a clonable handle for interacting with the orchestrator from
/// multiple tasks/threads.
pub struct Orchestrator {
    state: Arc<OrchestratorState>,
}

impl Orchestrator {
    /// Create a new orchestrator with default configuration.
    pub fn new() -> Self {
        Self::with_config(OrchestratorConfig::default())
    }

    /// Create a new orchestrator with explicit configuration.
    pub fn with_config(config: OrchestratorConfig) -> Self {
        let state = Arc::new(OrchestratorState::new(&config));
        state.emit(EventKind::OrchestratorStarted);
        tracing::info!("Orchestrator started");
        Self { state }
    }

    /// Return a clonable handle to the orchestrator.
    pub fn handle(&self) -> OrchestratorHandle {
        OrchestratorHandle {
            state: Arc::clone(&self.state),
        }
    }

    /// Subscribe to the event bus.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.state.event_tx.subscribe()
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// A cheaply-clonable handle used to interact with the orchestrator from
/// application code, plugins, and CLI commands.
#[derive(Clone)]
pub struct OrchestratorHandle {
    state: Arc<OrchestratorState>,
}

impl OrchestratorHandle {
    // ── Agent management ───────────────────────────────────────────────────

    /// Register a new agent with the orchestrator.
    ///
    /// The agent starts in the `Inactive` state. Call
    /// [`set_agent_ready`](Self::set_agent_ready) when the agent is prepared
    /// to receive tasks.
    pub fn register_agent(&self, descriptor: AgentDescriptor) -> SwarmResult<AgentId> {
        let id = self.state.registry.register(descriptor.clone())?;
        self.state.supervision.register_root(id);
        self.state.emit(EventKind::AgentRegistered {
            agent_id: id,
            name: descriptor.name.clone(),
        });
        tracing::info!(agent_id = %id, name = descriptor.name, "Agent registered");
        Ok(id)
    }

    /// Mark an agent as ready to receive tasks.
    pub fn set_agent_ready(&self, id: AgentId) -> SwarmResult<()> {
        let previous = self.state.registry.get(&id)?.status.label().to_string();
        self.state
            .registry
            .update_status(&id, AgentStatus::Ready)?;
        self.state.emit(EventKind::AgentStatusChanged {
            agent_id: id,
            previous,
            current: "ready".into(),
        });
        Ok(())
    }

    /// Deregister an agent. If the agent has a supervisor, it is removed from
    /// that supervisor's subordinate list.
    pub fn deregister_agent(&self, id: AgentId) -> SwarmResult<()> {
        self.state.registry.deregister(&id)?;
        self.state.supervision.deregister(&id);
        self.state.emit(EventKind::AgentDeregistered { agent_id: id });
        tracing::info!(agent_id = %id, "Agent deregistered");
        Ok(())
    }

    /// Register an agent under a supervisor in the supervision tree.
    pub fn set_supervisor(
        &self,
        agent_id: AgentId,
        supervisor_id: AgentId,
    ) -> SwarmResult<()> {
        self.state
            .supervision
            .register_under(agent_id, supervisor_id)
    }

    // ── Task management ────────────────────────────────────────────────────

    /// Submit a new task to the orchestrator.
    ///
    /// The task is validated, stored, and placed in the priority queue.
    /// Returns the new task's ID.
    pub fn submit_task(&self, spec: TaskSpec) -> SwarmResult<TaskId> {
        if spec.name.trim().is_empty() {
            return Err(SwarmError::InvalidTaskSpec {
                reason: "task name must not be empty".into(),
            });
        }
        let task = Task::new(spec);
        let task_id = task.id;
        self.state.tasks.insert(task_id, task.clone());
        self.state.task_queue.enqueue(task.clone())?;
        self.state.emit(EventKind::TaskSubmitted {
            task_id,
            name: task.spec.name.clone(),
        });
        tracing::info!(task_id = %task_id, name = task.spec.name, "Task submitted");
        Ok(task_id)
    }

    /// Attempt to schedule the next pending task.
    ///
    /// Returns `Some(TaskId)` if a task was successfully scheduled, `None` if
    /// the queue is empty or no agents are available.
    pub fn try_schedule_next(&self) -> SwarmResult<Option<TaskId>> {
        let task = match self.state.task_queue.peek() {
            Some(t) => t,
            None => return Ok(None),
        };

        match self.state.scheduler.schedule(&task)? {
            SchedulingDecision::Assigned { task_id, agent_id } => {
                // Remove from queue, update task status.
                self.state.task_queue.remove(&task_id)?;
                let mut t = self
                    .state
                    .tasks
                    .get_mut(&task_id)
                    .ok_or(SwarmError::TaskNotFound { id: task_id })?;
                t.schedule(agent_id).map_err(|s| SwarmError::AgentInvalidState {
                    id: agent_id,
                    reason: format!("task in state {}", s.label()),
                })?;
                // Mark agent as busy.
                self.state
                    .registry
                    .update_status(&agent_id, AgentStatus::Busy { current_task: task_id })?;
                self.state.emit(EventKind::TaskScheduled { task_id, agent_id });
                tracing::info!(task_id = %task_id, agent_id = %agent_id, "Task scheduled");
                Ok(Some(task_id))
            }
            SchedulingDecision::NoCapableAgent { .. } => Ok(None),
        }
    }

    /// Record that an agent has started executing a task.
    pub fn record_task_started(&self, task_id: TaskId, agent_id: AgentId) -> SwarmResult<()> {
        let mut t = self
            .state
            .tasks
            .get_mut(&task_id)
            .ok_or(SwarmError::TaskNotFound { id: task_id })?;
        t.start_running(agent_id).map_err(|s| SwarmError::AgentInvalidState {
            id: agent_id,
            reason: format!("task in state {}", s.label()),
        })?;
        self.state.emit(EventKind::TaskStarted { task_id, agent_id });
        Ok(())
    }

    /// Record that a task completed successfully.
    pub fn record_task_completed(
        &self,
        task_id: TaskId,
        agent_id: AgentId,
        output: serde_json::Value,
    ) -> SwarmResult<()> {
        let mut t = self
            .state
            .tasks
            .get_mut(&task_id)
            .ok_or(SwarmError::TaskNotFound { id: task_id })?;
        t.complete(output);
        self.state
            .registry
            .update_status(&agent_id, AgentStatus::Ready)?;
        self.state.registry.record_task_completed(&agent_id);
        self.state.emit(EventKind::TaskCompleted { task_id });
        tracing::info!(task_id = %task_id, agent_id = %agent_id, "Task completed");
        Ok(())
    }

    /// Record that a task failed.
    pub fn record_task_failed(
        &self,
        task_id: TaskId,
        agent_id: AgentId,
        reason: impl Into<String>,
    ) -> SwarmResult<()> {
        let reason = reason.into();
        let mut t = self
            .state
            .tasks
            .get_mut(&task_id)
            .ok_or(SwarmError::TaskNotFound { id: task_id })?;
        t.fail(reason.clone());
        self.state
            .registry
            .update_status(&agent_id, AgentStatus::Ready)?;
        self.state.registry.record_task_failed(&agent_id);
        self.state.emit(EventKind::TaskFailed {
            task_id,
            reason: reason.clone(),
        });
        tracing::warn!(task_id = %task_id, agent_id = %agent_id, reason = reason, "Task failed");
        Ok(())
    }

    /// Record that a task timed out while executing.
    pub fn record_task_timed_out(&self, task_id: TaskId, agent_id: AgentId) -> SwarmResult<()> {
        let mut t = self
            .state
            .tasks
            .get_mut(&task_id)
            .ok_or(SwarmError::TaskNotFound { id: task_id })?;
        t.time_out();
        self.state
            .registry
            .update_status(&agent_id, AgentStatus::Ready)?;
        self.state.registry.record_task_failed(&agent_id);
        self.state.emit(EventKind::TaskTimedOut { task_id });
        tracing::warn!(task_id = %task_id, agent_id = %agent_id, "Task timed out");
        Ok(())
    }

    /// Cancel a pending task by removing it from the queue.
    pub fn cancel_task(&self, task_id: TaskId) -> SwarmResult<()> {
        // Only pending (queued) tasks can be cancelled this way.
        self.state.task_queue.remove(&task_id)?;
        if let Some(mut t) = self.state.tasks.get_mut(&task_id) {
            t.status = TaskStatus::Cancelled {
                cancelled_at: swarm_core::types::now(),
                reason: None,
            };
        }
        self.state.emit(EventKind::TaskCancelled { task_id });
        Ok(())
    }

    // ── Queries ────────────────────────────────────────────────────────────

    /// Retrieve a snapshot of a task.
    pub fn get_task(&self, task_id: &TaskId) -> SwarmResult<Task> {
        self.state
            .tasks
            .get(task_id)
            .map(|t| t.clone())
            .ok_or_else(|| SwarmError::TaskNotFound { id: *task_id })
    }

    /// Return the current number of pending tasks in the queue.
    pub fn pending_task_count(&self) -> usize {
        self.state.task_queue.len()
    }

    /// Return a list of all registered agent records.
    pub fn list_agents(&self) -> Vec<crate::registry::AgentRecord> {
        self.state.registry.all_agents()
    }

    /// Subscribe to the orchestrator event bus.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.state.event_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind},
        capability::{Capability, CapabilitySet},
        event::EventKind,
        task::TaskSpec,
    };

    fn make_worker(caps: CapabilitySet) -> AgentDescriptor {
        AgentDescriptor::new("worker-1", AgentKind::Worker, caps)
    }

    fn orchestrator_with_ready_worker() -> (OrchestratorHandle, AgentId) {
        let orch = Orchestrator::new();
        let handle = orch.handle();
        let desc = make_worker(CapabilitySet::new());
        let agent_id = handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();
        (handle, agent_id)
    }

    #[test]
    fn submit_and_schedule_task() {
        let (handle, agent_id) = orchestrator_with_ready_worker();

        let spec = TaskSpec::new("test-task", serde_json::json!({"x": 1}));
        let task_id = handle.submit_task(spec).unwrap();

        assert_eq!(handle.pending_task_count(), 1);

        let scheduled = handle.try_schedule_next().unwrap();
        assert_eq!(scheduled, Some(task_id));
        assert_eq!(handle.pending_task_count(), 0);

        let task = handle.get_task(&task_id).unwrap();
        assert!(matches!(task.status, TaskStatus::Scheduled { assigned_to } if assigned_to == agent_id));
    }

    #[test]
    fn task_not_scheduled_when_no_capable_agent() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        // Register an agent with no capabilities
        let desc = make_worker(CapabilitySet::new());
        let agent_id = handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        // Submit a task requiring a capability the agent doesn't have
        let mut spec = TaskSpec::new("task", serde_json::json!({}));
        spec.required_capabilities = {
            let mut c = CapabilitySet::new();
            c.add(Capability::new("image-analysis"));
            c
        };
        handle.submit_task(spec).unwrap();

        let result = handle.try_schedule_next().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn full_task_lifecycle() {
        let (handle, agent_id) = orchestrator_with_ready_worker();

        let task_id = handle.submit_task(TaskSpec::new("t", serde_json::json!({}))).unwrap();
        handle.try_schedule_next().unwrap();
        handle.record_task_started(task_id, agent_id).unwrap();
        handle.record_task_completed(task_id, agent_id, serde_json::json!({"done": true})).unwrap();

        let task = handle.get_task(&task_id).unwrap();
        assert!(task.status.is_terminal());
        assert_eq!(task.status.label(), "completed");
    }

    #[test]
    fn invalid_task_name_rejected() {
        let (handle, _) = orchestrator_with_ready_worker();
        let spec = TaskSpec::new("   ", serde_json::json!({}));
        assert!(handle.submit_task(spec).is_err());
    }

    #[test]
    fn set_agent_ready_emits_actual_previous_status() {
        let orch = Orchestrator::new();
        let mut rx = orch.subscribe();
        let handle = orch.handle();
        let agent_id = handle
            .register_agent(make_worker(CapabilitySet::new()))
            .unwrap();

        handle.set_agent_ready(agent_id).unwrap();

        loop {
            let event = rx.try_recv().unwrap();
            if let EventKind::AgentStatusChanged { agent_id: event_agent, previous, current } = event.kind {
                assert_eq!(event_agent, agent_id);
                assert_eq!(previous, "inactive");
                assert_eq!(current, "ready");
                break;
            }
        }
    }

    #[test]
    fn record_task_started_requires_existing_task() {
        let (handle, agent_id) = orchestrator_with_ready_worker();
        let missing_task_id = TaskId::new();
        assert!(matches!(
            handle.record_task_started(missing_task_id, agent_id),
            Err(SwarmError::TaskNotFound { id }) if id == missing_task_id
        ));
    }
}
