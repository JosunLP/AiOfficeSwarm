//! The top-level orchestrator: coordinates agent registry, scheduler,
//! supervision, and event bus.
//!
//! The [`Orchestrator`] is the single control-plane component that glues
//! everything together. Users interact with it via an [`OrchestratorHandle`]
//! which exposes an async API.

use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;

use swarm_core::{
    agent::{AgentDescriptor, AgentStatus},
    error::{SwarmError, SwarmResult},
    event::{Event, EventEnvelope, EventKind},
    identity::{AgentId, TaskId},
    policy::PolicyContext,
    task::{Task, TaskSpec, TaskStatus},
};
use swarm_policy::PolicyEngine;

use crate::{
    registry::AgentRegistry,
    scheduler::{Scheduler, SchedulingDecision},
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
    /// Default task timeout applied when submitted tasks leave `timeout` unset.
    /// `None` disables the default deadline.
    pub default_task_timeout: Option<Duration>,
    /// Maximum number of tasks that may be scheduled or running at once across
    /// the whole swarm.
    pub max_concurrent_tasks: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            event_channel_capacity: 1024,
            max_dispatch_per_tick: 16,
            default_task_timeout: Some(Duration::from_secs(300)),
            max_concurrent_tasks: 256,
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
    policy_engine: Option<PolicyEngine>,
    default_task_timeout: Option<Duration>,
    max_concurrent_tasks: usize,
}

impl OrchestratorState {
    fn new(config: &OrchestratorConfig, policy_engine: Option<PolicyEngine>) -> Self {
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
            policy_engine,
            default_task_timeout: config.default_task_timeout,
            max_concurrent_tasks: config.max_concurrent_tasks,
        }
    }

    fn emit(&self, kind: EventKind) {
        let ev = EventEnvelope::new(kind);
        // It's OK if there are no subscribers.
        let _ = self.event_tx.send(ev);
    }

    fn in_flight_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|entry| {
                matches!(
                    entry.value().status,
                    TaskStatus::Scheduled { .. } | TaskStatus::Running { .. }
                )
            })
            .count()
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
        let state = Arc::new(OrchestratorState::new(&config, None));
        state.emit(EventKind::OrchestratorStarted);
        tracing::info!("Orchestrator started");
        Self { state }
    }

    /// Create a new orchestrator with explicit configuration and task admission
    /// policy enforcement.
    pub fn with_config_and_policy_engine(
        config: OrchestratorConfig,
        policy_engine: PolicyEngine,
    ) -> Self {
        let state = Arc::new(OrchestratorState::new(&config, Some(policy_engine)));
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
        tracing::info!(agent_id = %id, name = %descriptor.name, "Agent registered");
        Ok(id)
    }

    /// Mark an agent as ready to receive tasks.
    pub fn set_agent_ready(&self, id: AgentId) -> SwarmResult<()> {
        let previous = self.state.registry.get(&id)?.status.label().to_string();
        self.state.registry.update_status(&id, AgentStatus::Ready)?;
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
        self.state
            .emit(EventKind::AgentDeregistered { agent_id: id });
        tracing::info!(agent_id = %id, "Agent deregistered");
        Ok(())
    }

    /// Register an agent under a supervisor in the supervision tree.
    pub fn set_supervisor(&self, agent_id: AgentId, supervisor_id: AgentId) -> SwarmResult<()> {
        self.state
            .supervision
            .register_under(agent_id, supervisor_id)
    }

    // ── Task management ────────────────────────────────────────────────────

    /// Submit a new task to the orchestrator.
    ///
    /// The task is validated, stored, and placed in the priority queue.
    /// Returns the new task's ID.
    pub async fn submit_task(&self, mut spec: TaskSpec) -> SwarmResult<TaskId> {
        if spec.timeout.is_none() {
            spec.timeout = self.state.default_task_timeout;
        }
        spec.validate()?;
        self.enforce_task_policy("submit_task", &spec).await?;
        let task = Task::new(spec);
        let task_id = task.id;
        self.state.tasks.insert(task_id, task.clone());
        self.state.task_queue.enqueue(task.clone())?;
        self.state.emit(EventKind::TaskSubmitted {
            task_id,
            name: task.spec.name.clone(),
        });
        tracing::info!(task_id = %task_id, name = %task.spec.name, "Task submitted");
        Ok(task_id)
    }

    /// Attempt to schedule the next pending task.
    ///
    /// Returns `Some(TaskId)` if a task was successfully scheduled, `None` if
    /// the queue is empty or no agents are available.
    pub async fn try_schedule_next(&self) -> SwarmResult<Option<TaskId>> {
        if self.state.in_flight_task_count() >= self.state.max_concurrent_tasks {
            tracing::debug!(
                max_concurrent_tasks = self.state.max_concurrent_tasks,
                "global concurrency limit reached; delaying scheduling"
            );
            return Ok(None);
        }

        let task = match self.state.task_queue.peek() {
            Some(t) => t,
            None => return Ok(None),
        };
        self.enforce_task_policy("schedule_task", &task.spec)
            .await?;

        match self.state.scheduler.schedule(&task)? {
            SchedulingDecision::Assigned { task_id, agent_id } => {
                // Remove from queue, update task status.
                self.state.task_queue.remove(&task_id)?;
                let mut t = self
                    .state
                    .tasks
                    .get_mut(&task_id)
                    .ok_or(SwarmError::TaskNotFound { id: task_id })?;
                t.schedule(agent_id)
                    .map_err(|s| SwarmError::AgentInvalidState {
                        id: agent_id,
                        reason: format!("task in state {}", s.label()),
                    })?;
                // Mark agent as busy.
                self.state.registry.update_status(
                    &agent_id,
                    AgentStatus::Busy {
                        current_task: task_id,
                    },
                )?;
                self.state
                    .emit(EventKind::TaskScheduled { task_id, agent_id });
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
        t.start_running(agent_id)
            .map_err(|s| SwarmError::AgentInvalidState {
                id: agent_id,
                reason: format!("task in state {}", s.label()),
            })?;
        self.state
            .emit(EventKind::TaskStarted { task_id, agent_id });
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
            .ok_or(SwarmError::TaskNotFound { id: *task_id })
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

    /// Publish an event onto the orchestrator event bus.
    ///
    /// This is primarily intended for sibling runtime components that need to
    /// surface memory, learning, provider, or personality events without
    /// taking a direct dependency on the orchestrator's internal state.
    pub fn publish_event(&self, kind: EventKind) {
        self.state.emit(kind);
    }

    async fn enforce_task_policy(&self, action: &str, spec: &TaskSpec) -> SwarmResult<()> {
        let Some(policy_engine) = &self.state.policy_engine else {
            return Ok(());
        };

        let priority = match spec.priority {
            swarm_core::task::TaskPriority::Low => "low",
            swarm_core::task::TaskPriority::Normal => "normal",
            swarm_core::task::TaskPriority::High => "high",
            swarm_core::task::TaskPriority::Critical => "critical",
        };
        let mut context = PolicyContext::new(action, "orchestrator", &spec.name);
        context.attributes = serde_json::json!({
            "task_name": spec.name,
            "priority": priority,
            "required_capabilities": spec
                .required_capabilities
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "metadata": spec.metadata.0.clone(),
        });

        match policy_engine.enforce(&context).await {
            Ok(()) => {
                self.state.emit(EventKind::PolicyEvaluated {
                    action: context.action,
                    subject: context.subject,
                    decision: "allowed".into(),
                });
                Ok(())
            }
            Err(error) => {
                self.state.emit(EventKind::PolicyEvaluated {
                    action: context.action,
                    subject: context.subject,
                    decision: "denied".into(),
                });
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind},
        capability::{Capability, CapabilitySet},
        event::EventKind,
        policy::PolicyContext,
        task::{TaskSpec, TaskStatus},
    };
    use swarm_policy::ActionAllowlistPolicy;

    const POLICY_BY_ACTION_ID: &str = "00000000-0000-0000-0000-000000000123";

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

    #[tokio::test]
    async fn submit_and_schedule_task() {
        let (handle, agent_id) = orchestrator_with_ready_worker();

        let spec = TaskSpec::new("test-task", serde_json::json!({"x": 1}));
        let task_id = handle.submit_task(spec).await.unwrap();

        assert_eq!(handle.pending_task_count(), 1);

        let scheduled = handle.try_schedule_next().await.unwrap();
        assert_eq!(scheduled, Some(task_id));
        assert_eq!(handle.pending_task_count(), 0);

        let task = handle.get_task(&task_id).unwrap();
        assert!(
            matches!(task.status, TaskStatus::Scheduled { assigned_to } if assigned_to == agent_id)
        );
    }

    #[tokio::test]
    async fn task_not_scheduled_when_no_capable_agent() {
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
        handle.submit_task(spec).await.unwrap();

        let result = handle.try_schedule_next().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn full_task_lifecycle() {
        let (handle, agent_id) = orchestrator_with_ready_worker();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({})))
            .await
            .unwrap();
        handle.try_schedule_next().await.unwrap();
        handle.record_task_started(task_id, agent_id).unwrap();
        handle
            .record_task_completed(task_id, agent_id, serde_json::json!({"done": true}))
            .unwrap();

        let task = handle.get_task(&task_id).unwrap();
        assert!(task.status.is_terminal());
        assert_eq!(task.status.label(), "completed");
    }

    #[tokio::test]
    async fn invalid_task_name_rejected() {
        let (handle, _) = orchestrator_with_ready_worker();
        let spec = TaskSpec::new("   ", serde_json::json!({}));
        assert!(handle.submit_task(spec).await.is_err());
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
            if let EventKind::AgentStatusChanged {
                agent_id: event_agent,
                previous,
                current,
            } = event.kind
            {
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

    #[tokio::test]
    async fn submit_task_applies_default_timeout_when_missing() {
        let orch = Orchestrator::with_config(OrchestratorConfig {
            default_task_timeout: Some(Duration::from_secs(42)),
            ..OrchestratorConfig::default()
        });
        let handle = orch.handle();
        let desc = make_worker(CapabilitySet::new());
        let agent_id = handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let mut spec = TaskSpec::new("timeout-from-config", serde_json::json!({}));
        spec.timeout = None;

        let task_id = handle.submit_task(spec).await.unwrap();
        let task = handle.get_task(&task_id).unwrap();

        assert_eq!(task.spec.timeout, Some(Duration::from_secs(42)));
    }

    #[tokio::test]
    async fn zero_default_timeout_preserves_timeout_none() {
        let orch = Orchestrator::with_config(OrchestratorConfig {
            default_task_timeout: None,
            ..OrchestratorConfig::default()
        });
        let handle = orch.handle();
        let desc = make_worker(CapabilitySet::new());
        let agent_id = handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let mut spec = TaskSpec::new("no-timeout", serde_json::json!({}));
        spec.timeout = None;

        let task_id = handle.submit_task(spec).await.unwrap();
        let task = handle.get_task(&task_id).unwrap();

        assert_eq!(task.spec.timeout, None);
    }

    #[tokio::test]
    async fn scheduling_respects_global_concurrency_limit() {
        let orch = Orchestrator::with_config(OrchestratorConfig {
            max_concurrent_tasks: 1,
            ..OrchestratorConfig::default()
        });
        let handle = orch.handle();

        let worker_a = AgentDescriptor::new("worker-a", AgentKind::Worker, CapabilitySet::new());
        let worker_a_id = worker_a.id;
        let worker_b = AgentDescriptor::new("worker-b", AgentKind::Worker, CapabilitySet::new());
        let worker_b_id = worker_b.id;

        handle.register_agent(worker_a).unwrap();
        handle.register_agent(worker_b).unwrap();
        handle.set_agent_ready(worker_a_id).unwrap();
        handle.set_agent_ready(worker_b_id).unwrap();

        let first_task_id = handle
            .submit_task(TaskSpec::new("first", serde_json::json!({})))
            .await
            .unwrap();
        let second_task_id = handle
            .submit_task(TaskSpec::new("second", serde_json::json!({})))
            .await
            .unwrap();

        assert_eq!(
            handle.try_schedule_next().await.unwrap(),
            Some(first_task_id)
        );
        assert_eq!(handle.try_schedule_next().await.unwrap(), None);

        let assigned_agent_id = match handle.get_task(&first_task_id).unwrap().status {
            TaskStatus::Scheduled { assigned_to } => assigned_to,
            status => panic!("expected scheduled task, got {}", status.label()),
        };

        handle
            .record_task_started(first_task_id, assigned_agent_id)
            .unwrap();
        handle
            .record_task_completed(
                first_task_id,
                assigned_agent_id,
                serde_json::json!({"done": true}),
            )
            .unwrap();

        assert_eq!(
            handle.try_schedule_next().await.unwrap(),
            Some(second_task_id)
        );
    }

    #[tokio::test]
    async fn attached_policy_engine_controls_submission_and_scheduling() {
        let policy_engine = PolicyEngine::deny_by_default();
        policy_engine
            .register(Arc::new(ActionAllowlistPolicy::new(
                "task-policy",
                ["submit_task", "schedule_task"],
            )))
            .await;
        let orch = Orchestrator::with_config_and_policy_engine(
            OrchestratorConfig::default(),
            policy_engine,
        );
        let handle = orch.handle();
        let agent_id = handle
            .register_agent(make_worker(CapabilitySet::new()))
            .unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("policy-task", serde_json::json!({})))
            .await
            .unwrap();

        assert_eq!(handle.try_schedule_next().await.unwrap(), Some(task_id));
    }

    #[tokio::test]
    async fn submission_policy_denial_rejects_task_and_emits_event() {
        let policy_engine = PolicyEngine::deny_by_default();
        policy_engine
            .register(Arc::new(ActionAllowlistPolicy::new(
                "schedule-only",
                ["schedule_task"],
            )))
            .await;
        let orch = Orchestrator::with_config_and_policy_engine(
            OrchestratorConfig::default(),
            policy_engine,
        );
        let mut rx = orch.subscribe();
        let handle = orch.handle();

        let error = handle
            .submit_task(TaskSpec::new("blocked-task", serde_json::json!({})))
            .await
            .unwrap_err();
        assert!(
            matches!(error, SwarmError::PolicyViolation { action, .. } if action == "submit_task")
        );
        assert_eq!(handle.pending_task_count(), 0);

        loop {
            let event = rx.try_recv().unwrap();
            if let EventKind::PolicyEvaluated {
                action,
                subject,
                decision,
            } = event.kind
            {
                assert_eq!(action, "submit_task");
                assert_eq!(subject, "orchestrator");
                assert_eq!(decision, "denied");
                break;
            }
        }
    }

    #[tokio::test]
    async fn scheduling_policy_denial_keeps_task_pending() {
        let policy_engine = PolicyEngine::deny_by_default();
        policy_engine
            .register(Arc::new(PolicyByAction {
                submit_allowed: true,
                schedule_allowed: false,
            }))
            .await;
        let orch = Orchestrator::with_config_and_policy_engine(
            OrchestratorConfig::default(),
            policy_engine,
        );
        let handle = orch.handle();
        let agent_id = handle
            .register_agent(make_worker(CapabilitySet::new()))
            .unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("blocked-schedule", serde_json::json!({})))
            .await
            .unwrap();

        let error = handle.try_schedule_next().await.unwrap_err();
        assert!(
            matches!(error, SwarmError::PolicyViolation { action, .. } if action == "schedule_task")
        );
        assert_eq!(handle.pending_task_count(), 1);
        assert!(matches!(
            handle.get_task(&task_id).unwrap().status,
            TaskStatus::Pending
        ));
    }

    struct PolicyByAction {
        submit_allowed: bool,
        schedule_allowed: bool,
    }

    #[async_trait::async_trait]
    impl swarm_core::policy::Policy for PolicyByAction {
        fn id(&self) -> swarm_core::identity::PolicyId {
            POLICY_BY_ACTION_ID.parse().expect("test UUID should parse")
        }

        fn name(&self) -> &str {
            "policy-by-action"
        }

        async fn evaluate(
            &self,
            context: &PolicyContext,
        ) -> SwarmResult<swarm_core::policy::PolicyOutcome> {
            match context.action.as_str() {
                "submit_task" if self.submit_allowed => {
                    Ok(swarm_core::policy::PolicyOutcome::Allow)
                }
                "schedule_task" if self.schedule_allowed => {
                    Ok(swarm_core::policy::PolicyOutcome::Allow)
                }
                _ => Ok(swarm_core::policy::PolicyOutcome::Deny {
                    reason: format!("{} blocked", context.action),
                }),
            }
        }
    }
}
