//! `swarm demo` sub-command: runs an in-process demonstration of the framework.
//!
//! The demo creates a small swarm with:
//! - Two Worker agents
//!
//! It submits several tasks, runs them through the runtime, and prints a
//! summary showing the framework in action.

use clap::Args;
use async_trait::async_trait;

use swarm_config::SwarmConfig;
use swarm_core::{
    agent::{Agent, AgentDescriptor, AgentKind},
    capability::{Capability, CapabilitySet},
    error::SwarmResult,
    task::{Task, TaskSpec, TaskStatus},
};
use swarm_orchestrator::Orchestrator;
use swarm_runtime::TaskRunner;
use swarm_telemetry::Metrics;

fn orchestrator_config_from_swarm(
    config: &swarm_config::model::OrchestratorConfig,
) -> swarm_orchestrator::OrchestratorConfig {
    swarm_orchestrator::OrchestratorConfig {
        event_channel_capacity: config.event_channel_capacity,
        max_dispatch_per_tick: config.max_dispatch_per_tick,
    }
}

/// Demo command arguments.
#[derive(Args)]
pub struct DemoArgs {
    /// Number of tasks to submit in the demo.
    #[arg(short, long, default_value = "6")]
    pub task_count: usize,
}

// ─── Demo agent implementations ───────────────────────────────────────────────

/// A simple worker agent that echoes its input back as output.
struct EchoWorker {
    descriptor: AgentDescriptor,
}

#[async_trait]
impl Agent for EchoWorker {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        tracing::info!(
            agent = %self.descriptor.name,
            task = %task.spec.name,
            "EchoWorker executing task"
        );
        // Simulate a small amount of work.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        Ok(serde_json::json!({
            "agent": &self.descriptor.name,
            "task": task.spec.name,
            "echo": task.spec.input,
        }))
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

// ─── Demo runner ──────────────────────────────────────────────────────────────

pub async fn run(args: DemoArgs, config: &SwarmConfig) -> anyhow::Result<()> {
    println!("=== AiOfficeSwarm Demo ===");
    println!("Starting a demo swarm with 2 worker agents...\n");

    let orch = Orchestrator::with_config(orchestrator_config_from_swarm(&config.orchestrator));
    let handle = orch.handle();
    let metrics = Metrics::new();

    // Register agents.
    let mut capabilities = CapabilitySet::new();
    capabilities.add(Capability::new("text-processing"));

    let worker1_desc = AgentDescriptor::new("Worker-Alpha", AgentKind::Worker, capabilities.clone());
    let worker2_desc = AgentDescriptor::new("Worker-Beta", AgentKind::Worker, capabilities.clone());

    let w1_id = handle.register_agent(worker1_desc.clone())?;
    let w2_id = handle.register_agent(worker2_desc.clone())?;
    handle.set_agent_ready(w1_id)?;
    handle.set_agent_ready(w2_id)?;

    metrics.inc_agents_registered();
    metrics.inc_agents_registered();

    println!("✓ Registered Worker-Alpha ({})", w1_id);
    println!("✓ Registered Worker-Beta  ({})", w2_id);
    println!();

    // Submit tasks.
    let mut task_ids = Vec::new();
    for i in 1..=args.task_count {
        let spec = TaskSpec::new(
            format!("task-{}", i),
            serde_json::json!({ "message": format!("Hello from task {}", i) }),
        );
        let task_id = handle.submit_task(spec)?;
        task_ids.push(task_id);
        metrics.inc_tasks_submitted();
    }
    println!("✓ Submitted {} tasks\n", args.task_count);

    // Create runners for each worker.
    let mut runner1 = TaskRunner::new(
        Box::new(EchoWorker { descriptor: worker1_desc }),
        handle.clone(),
    );
    let mut runner2 = TaskRunner::new(
        Box::new(EchoWorker { descriptor: worker2_desc }),
        handle.clone(),
    );

    // Schedule and execute all tasks.
    println!("Processing tasks...");
    let mut completed = 0;
    let mut failed = 0;

    for _ in 0..args.task_count {
        // Schedule the next task.
        if let Some(task_id) = handle.try_schedule_next()? {
            let task = handle.get_task(&task_id)?;
            let assigned_to = match task.status {
                TaskStatus::Scheduled { assigned_to } => assigned_to,
                _ => anyhow::bail!("scheduled task {task_id} was not in the scheduled state"),
            };

            let result = if assigned_to == runner1.agent_id() {
                runner1.run_task(task).await
            } else if assigned_to == runner2.agent_id() {
                runner2.run_task(task).await
            } else {
                anyhow::bail!("task {task_id} was assigned to unknown agent {assigned_to}");
            };

            match result {
                Ok(output) => {
                    completed += 1;
                    metrics.inc_tasks_completed();
                    println!("  ✓ task-{} completed: {}", completed + failed, output["echo"]["message"]);
                }
                Err(e) => {
                    failed += 1;
                    metrics.inc_tasks_failed();
                    println!("  ✗ task failed: {}", e);
                }
            }
        }
    }

    println!();
    println!("=== Demo Complete ===");
    let snap = metrics.snapshot();
    println!("  Tasks submitted:  {}", snap.tasks_submitted);
    println!("  Tasks completed:  {}", snap.tasks_completed);
    println!("  Tasks failed:     {}", snap.tasks_failed);
    println!("  Agents active:    {}", snap.agents_registered);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::orchestrator_config_from_swarm;
    use swarm_config::SwarmConfig;

    #[test]
    fn demo_uses_loaded_orchestrator_config() {
        let mut config = SwarmConfig::default();
        config.orchestrator.event_channel_capacity = 32;
        config.orchestrator.max_dispatch_per_tick = 7;

        let orchestrator_config = orchestrator_config_from_swarm(&config.orchestrator);

        assert_eq!(orchestrator_config.event_channel_capacity, 32);
        assert_eq!(orchestrator_config.max_dispatch_per_tick, 7);
    }
}
