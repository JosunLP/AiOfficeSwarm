//! # Basic Swarm Example
//!
//! This example demonstrates the core usage of the AiOfficeSwarm framework:
//!
//! 1. Configuring telemetry.
//! 2. Creating an orchestrator.
//! 3. Registering agents with different capabilities.
//! 4. Submitting tasks and dispatching them.
//! 5. Collecting results.
//! 6. Loading and invoking a plugin.
//! 7. Configuring the policy engine for application-managed admission control.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::info;

use swarm_config::ConfigLoader;
use swarm_core::{
    agent::{Agent, AgentDescriptor, AgentKind},
    capability::{Capability, CapabilitySet},
    error::SwarmResult,
    policy::PolicyContext,
    task::{Task, TaskPriority, TaskSpec, TaskStatus},
};
use swarm_orchestrator::Orchestrator;
use swarm_plugin::PluginHost;
use swarm_policy::{AllowAllPolicy, PolicyEngine};
use swarm_runtime::TaskRunner;
use swarm_telemetry::{init_tracing, Metrics};

// ─── Custom Agent Implementation ─────────────────────────────────────────────

/// A simple agent that processes text tasks.
struct TextProcessingAgent {
    descriptor: AgentDescriptor,
}

impl TextProcessingAgent {
    fn new(name: &str) -> Self {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::new("text-processing"));
        caps.add(Capability::new("summarization"));

        Self {
            descriptor: AgentDescriptor::new(name, AgentKind::Worker, caps),
        }
    }
}

#[async_trait]
impl Agent for TextProcessingAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        let text = task.spec.input["text"].as_str().unwrap_or("(no text)");
        // Simulate processing.
        let word_count = text.split_whitespace().count();
        info!(
            agent = %self.descriptor.name,
            task = task.spec.name,
            word_count,
            "Processing text task"
        );
        Ok(json!({
            "agent": &self.descriptor.name,
            "original_length": text.len(),
            "word_count": word_count,
            "summary": format!("[Summary of {} words]", word_count),
        }))
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

/// A data analysis agent.
struct DataAnalysisAgent {
    descriptor: AgentDescriptor,
}

impl DataAnalysisAgent {
    fn new(name: &str) -> Self {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::new("data-analysis"));
        caps.add(Capability::new("report-generation"));

        Self {
            descriptor: AgentDescriptor::new(name, AgentKind::Worker, caps),
        }
    }
}

fn summarize_values(values: &[f64]) -> (f64, f64, Option<f64>, Option<f64>) {
    let sum: f64 = values.iter().sum();
    let mean = if values.is_empty() { 0.0 } else { sum / values.len() as f64 };
    let max = values.iter().copied().reduce(f64::max);
    let min = values.iter().copied().reduce(f64::min);
    (sum, mean, max, min)
}

#[async_trait]
impl Agent for DataAnalysisAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        let values: Vec<f64> = task.spec.input["values"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_default();

        let (sum, mean, max, min) = summarize_values(&values);

        info!(
            agent = %self.descriptor.name,
            task = task.spec.name,
            n = values.len(),
            "Analyzing data"
        );

        Ok(json!({
            "agent": &self.descriptor.name,
            "n": values.len(),
            "sum": sum,
            "mean": mean,
            "max": max,
            "min": min,
        }))
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_values;

    #[test]
    fn summarize_values_returns_nullish_bounds_for_empty_input() {
        let (sum, mean, max, min) = summarize_values(&[]);

        assert_eq!(sum, 0.0);
        assert_eq!(mean, 0.0);
        assert_eq!(max, None);
        assert_eq!(min, None);
    }

    #[test]
    fn summarize_values_reports_numeric_bounds_for_non_empty_input() {
        let (sum, mean, max, min) = summarize_values(&[1.0, 4.0, 2.0]);

        assert_eq!(sum, 7.0);
        assert!((mean - (7.0 / 3.0)).abs() < 1e-12);
        assert_eq!(max, Some(4.0));
        assert_eq!(min, Some(1.0));
    }
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load configuration.
    let config = ConfigLoader::with_env_overrides(ConfigLoader::defaults());
    init_tracing(&config.telemetry);

    println!("╔═══════════════════════════════════════════════╗");
    println!("║       AiOfficeSwarm — Basic Example           ║");
    println!("╚═══════════════════════════════════════════════╝\n");

    // 2. Create the orchestrator.
    let orch = Orchestrator::new();
    let handle = orch.handle();
    let metrics = Metrics::new();

    // 3. Set up a permissive policy engine for this demo.
    let policy_engine = PolicyEngine::allow_by_default();
    policy_engine
        .register(Arc::new(AllowAllPolicy::new("demo-allow-all")))
        .await;

    // 4. Register agents.
    println!("── Registering Agents ──────────────────────────");
    let text_agent = TextProcessingAgent::new("TextProcessor-1");
    let text_agent_id = handle.register_agent(text_agent.descriptor().clone())?;
    handle.set_agent_ready(text_agent_id)?;
    metrics.inc_agents_registered();
    println!("  ✓ TextProcessor-1 registered (text-processing, summarization)");

    let data_agent = DataAnalysisAgent::new("DataAnalyst-1");
    let data_agent_id = handle.register_agent(data_agent.descriptor().clone())?;
    handle.set_agent_ready(data_agent_id)?;
    metrics.inc_agents_registered();
    println!("  ✓ DataAnalyst-1 registered (data-analysis, report-generation)");
    println!();

    // 5. Submit tasks with varying priorities.
    println!("── Submitting Tasks ────────────────────────────");

    let mut text_spec = TaskSpec::new(
        "summarize-report",
        json!({ "text": "The quarterly report shows strong performance across all business units. Revenue grew by 15% year-over-year driven by product innovation and geographic expansion." }),
    );
    text_spec.priority = TaskPriority::High;
    text_spec.required_capabilities = {
        let mut c = CapabilitySet::new();
        c.add(Capability::new("text-processing"));
        c
    };
    policy_engine
        .enforce(&PolicyContext::new("submit_task", "basic_swarm", "task-queue"))
        .await?;
    let _task1_id = handle.submit_task(text_spec)?;
    metrics.inc_tasks_submitted();
    println!("  ✓ Submitted: summarize-report (High priority)");

    let mut data_spec = TaskSpec::new(
        "analyze-sales",
        json!({ "values": [1200.5, 1350.0, 980.25, 1450.75, 1100.0, 1600.0] }),
    );
    data_spec.required_capabilities = {
        let mut c = CapabilitySet::new();
        c.add(Capability::new("data-analysis"));
        c
    };
    policy_engine
        .enforce(&PolicyContext::new("submit_task", "basic_swarm", "task-queue"))
        .await?;
    let _task2_id = handle.submit_task(data_spec)?;
    metrics.inc_tasks_submitted();
    println!("  ✓ Submitted: analyze-sales (Normal priority)");

    policy_engine
        .enforce(&PolicyContext::new("submit_task", "basic_swarm", "task-queue"))
        .await?;
    let _task3_id = handle.submit_task(TaskSpec::new(
        "summarize-meeting-notes",
        json!({ "text": "Team standup: sprint velocity is on track, two blockers identified in the backend integration module." }),
    ))?;
    metrics.inc_tasks_submitted();
    println!("  ✓ Submitted: summarize-meeting-notes (Normal priority)");
    println!();

    // 6. Schedule and execute tasks.
    println!("── Executing Tasks ─────────────────────────────");

    let mut text_runner = TaskRunner::new(Box::new(text_agent), handle.clone());
    let mut data_runner = TaskRunner::new(Box::new(data_agent), handle.clone());

    // Schedule and run task 1.
    if let Some(task_id) = handle.try_schedule_next()? {
        let task = handle.get_task(&task_id)?;
        let output = text_runner.run_task(task).await?;
        metrics.inc_tasks_completed();
        println!("  ✓ summarize-report → {:?}", output);
    }

    // Schedule and run task 2.
    if let Some(task_id) = handle.try_schedule_next()? {
        let task = handle.get_task(&task_id)?;
        let output = data_runner.run_task(task).await?;
        let mean = output["mean"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("expected numeric mean in analyze-sales output"))?;
        let max = output["max"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("expected numeric max in analyze-sales output"))?;
        metrics.inc_tasks_completed();
        println!("  ✓ analyze-sales    → mean={mean:.2}, max={max:.2}");
    }

    // Mark agents ready again for task 3.
    handle.set_agent_ready(text_runner.agent_id())?;
    handle.set_agent_ready(data_runner.agent_id())?;
    if let Some(task_id) = handle.try_schedule_next()? {
        let task = handle.get_task(&task_id)?;
        let assigned_to = match task.status {
            TaskStatus::Scheduled { assigned_to } => assigned_to,
            _ => anyhow::bail!("scheduled task {task_id} not in expected Scheduled state"),
        };
        let output = if assigned_to == text_runner.agent_id() {
            text_runner.run_task(task).await?
        } else {
            data_runner.run_task(task).await?
        };
        metrics.inc_tasks_completed();
        println!("  ✓ summarize-meeting → {:?}", output["summary"]);
    }
    println!();

    // 7. Load and invoke a plugin.
    println!("── Plugin Demonstration ────────────────────────");
    let plugin_host = PluginHost::new();
    let plugin_id = plugin_host
        .load(Box::new(example_integration::NotificationPlugin::new("#ops-alerts")))
        .await?;

    policy_engine
        .enforce(&PolicyContext::new(
            "invoke_plugin",
            "basic_swarm",
            plugin_id.to_string(),
        ))
        .await?;
    let result = plugin_host
        .invoke(
            &plugin_id,
            "send_notification",
            json!({
                "message": "All tasks completed successfully",
                "severity": "info"
            }),
        )
        .await?;
    metrics.inc_plugin_invocations();
    println!(
        "  ✓ Notification sent → delivered={}, channel={}",
        result["delivered"], result["channel"]
    );
    println!();

    // 8. Print summary.
    println!("── Summary ─────────────────────────────────────");
    let snap = metrics.snapshot();
    println!("  Tasks submitted:  {}", snap.tasks_submitted);
    println!("  Tasks completed:  {}", snap.tasks_completed);
    println!("  Tasks failed:     {}", snap.tasks_failed);
    println!("  Agents active:    {}", snap.agents_registered);
    println!("  Plugin invocations: {}", snap.plugin_invocations);

    println!("\nAll done! ✓");

    Ok(())
}
