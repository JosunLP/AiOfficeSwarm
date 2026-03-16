//! Task runner: drives a single agent's task execution loop with timeout
//! and circuit-breaker integration.
//!
//! The [`TaskRunner`] owns a boxed [`Agent`] and a reference to the
//! [`OrchestratorHandle`]. When `run_task` is called, it:
//!
//! 1. Checks the circuit breaker.
//! 2. Notifies the orchestrator that the task has started.
//! 3. Drives the agent's `execute` call under a timeout.
//! 4. Reports the outcome (success or failure) back to the orchestrator.
//! 5. Updates the circuit breaker.

use std::time::Duration;

use swarm_core::{
    agent::Agent,
    error::{SwarmError, SwarmResult},
    identity::AgentId,
    task::Task,
};
use swarm_orchestrator::OrchestratorHandle;

use crate::circuit_breaker::CircuitBreaker;

/// Drives a single agent through task execution.
pub struct TaskRunner {
    agent: Box<dyn Agent>,
    handle: OrchestratorHandle,
    circuit_breaker: CircuitBreaker,
}

impl TaskRunner {
    /// Create a new task runner for the given agent.
    pub fn new(agent: Box<dyn Agent>, handle: OrchestratorHandle) -> Self {
        let name = agent.descriptor().name.clone();
        Self {
            agent,
            handle,
            circuit_breaker: CircuitBreaker::new(name),
        }
    }

    /// Returns the ID of the agent managed by this runner.
    pub fn agent_id(&self) -> AgentId {
        self.agent.descriptor().id
    }

    /// Execute the given task, reporting results to the orchestrator.
    ///
    /// The task must already be in the `Scheduled` state (i.e., it has been
    /// assigned to this agent by the scheduler).
    pub async fn run_task(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        let task_id = task.id;
        let agent_id = self.agent_id();

        // Check circuit breaker before attempting execution.
        if let Err(error) = self.circuit_breaker.acquire() {
            self.handle
                .record_task_failed(task_id, agent_id, error.to_string())?;
            return Err(error);
        }

        // Tell the orchestrator execution is starting.
        // If this fails (e.g., wrong assigned agent), record the task as
        // failed and reset the agent so neither gets stuck.
        if let Err(start_err) = self.handle.record_task_started(task_id, agent_id) {
            if let Err(record_err) = self.handle.record_task_failed(
                task_id,
                agent_id,
                start_err.to_string(),
            ) {
                tracing::error!(
                    task_id = %task_id,
                    agent_id = %agent_id,
                    error = %record_err,
                    "failed to record task failure after record_task_started error"
                );
            }
            return Err(start_err);
        }

        // Determine timeout from the task spec.
        let timeout = task.spec.timeout.unwrap_or(Duration::from_secs(300));

        // Execute with timeout.
        let result = tokio::time::timeout(
            timeout,
            self.agent.execute(task),
        ).await;

        match result {
            Ok(Ok(output)) => {
                self.circuit_breaker.record_success();
                self.handle.record_task_completed(task_id, agent_id, output.clone())?;
                Ok(output)
            }
            Ok(Err(e)) => {
                self.circuit_breaker.record_failure();
                self.handle.record_task_failed(task_id, agent_id, e.to_string())?;
                Err(e)
            }
            Err(_elapsed) => {
                self.circuit_breaker.record_failure();
                self.handle.record_task_timed_out(task_id, agent_id)?;
                Err(SwarmError::TaskTimeout {
                    id: task_id,
                    elapsed_ms: timeout.as_millis() as u64,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind},
        capability::CapabilitySet,
        task::TaskSpec,
    };
    use swarm_orchestrator::Orchestrator;

    struct OkAgent { descriptor: AgentDescriptor }
    struct FailAgent { descriptor: AgentDescriptor }
    struct SlowAgent { descriptor: AgentDescriptor }

    #[async_trait]
    impl Agent for OkAgent {
        fn descriptor(&self) -> &AgentDescriptor { &self.descriptor }
        async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
            Ok(task.spec.input.clone())
        }
        async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
    }

    #[async_trait]
    impl Agent for FailAgent {
        fn descriptor(&self) -> &AgentDescriptor { &self.descriptor }
        async fn execute(&mut self, _task: Task) -> SwarmResult<serde_json::Value> {
            Err(SwarmError::Internal { reason: "agent failed".into() })
        }
        async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
    }

    #[async_trait]
    impl Agent for SlowAgent {
        fn descriptor(&self) -> &AgentDescriptor { &self.descriptor }
        async fn execute(&mut self, _task: Task) -> SwarmResult<serde_json::Value> {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(serde_json::json!({"slow": true}))
        }
        async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
    }


    #[tokio::test]
    async fn run_task_success() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("ok-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent { descriptor: desc.clone() };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle.submit_task(TaskSpec::new("t", serde_json::json!({"x": 1}))).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone());

        let output = runner.run_task(task).await.unwrap();
        assert_eq!(output, serde_json::json!({"x": 1}));

        let completed_task = handle.get_task(&task_id).unwrap();
        assert!(completed_task.status.is_terminal());
    }

    #[tokio::test]
    async fn run_task_failure_recorded() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("fail-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = FailAgent { descriptor: desc.clone() };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle.submit_task(TaskSpec::new("t", serde_json::json!({}))).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone());

        assert!(runner.run_task(task).await.is_err());

        let failed_task = handle.get_task(&task_id).unwrap();
        assert_eq!(failed_task.status.label(), "failed");
    }

    #[tokio::test]
    async fn run_task_timeout_recorded_as_timed_out() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("slow-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = SlowAgent { descriptor: desc.clone() };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let mut spec = TaskSpec::new("t", serde_json::json!({}));
        spec.timeout = Some(Duration::from_millis(5));
        let task_id = handle.submit_task(spec).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone());

        assert!(matches!(
            runner.run_task(task).await,
            Err(SwarmError::TaskTimeout { id, .. }) if id == task_id
        ));

        let timed_out_task = handle.get_task(&task_id).unwrap();
        assert_eq!(timed_out_task.status.label(), "timed_out");
    }

    #[tokio::test]
    async fn run_task_preserves_circuit_breaker_error_details() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("ok-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent { descriptor: desc.clone() };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle.submit_task(TaskSpec::new("t", serde_json::json!({"x": 1}))).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle);

        for _ in 0..5 {
            runner.circuit_breaker.record_failure();
        }

        assert!(matches!(
            runner.run_task(task).await,
            Err(SwarmError::Internal { reason })
                if reason.contains("circuit 'ok-worker' is open")
        ));

        let failed_task = runner.handle.get_task(&task_id).unwrap();
        assert_eq!(failed_task.status.label(), "failed");
        let agent_record = runner
            .handle
            .list_agents()
            .into_iter()
            .find(|record| record.descriptor.id == agent_id)
            .unwrap();
        assert_eq!(agent_record.status.label(), "ready");
    }
}
