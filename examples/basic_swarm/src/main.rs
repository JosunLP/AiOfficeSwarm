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
//! 7. Using the policy engine for admission control.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::info;

use swarm_config::ConfigLoader;
use swarm_core::{
    agent::{Agent, AgentDescriptor, AgentKind},
    capability::{Capability, CapabilitySet},
    error::SwarmResult,
    task::{Task, TaskPriority, TaskSpec},
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
            agent = self.descriptor.name,
            task = task.spec.name,
            word_count,
            "Processing text task"
        );
        Ok(json!({
            "agent": self.descriptor.name,
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

        let sum: f64 = values.iter().sum();
        let mean = if values.is_empty() { 0.0 } else { sum / values.len() as f64 };
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);

        info!(
            agent = self.descriptor.name,
            task = task.spec.name,
            n = values.len(),
            "Analyzing data"
        );

        Ok(json!({
            "agent": self.descriptor.name,
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
    let _task2_id = handle.submit_task(data_spec)?;
    metrics.inc_tasks_submitted();
    println!("  ✓ Submitted: analyze-sales (Normal priority)");

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
        metrics.inc_tasks_completed();
        println!("  ✓ analyze-sales    → mean={:.2}, max={:.2}", output["mean"], output["max"]);
    }

    // Mark agents ready again for task 3.
    handle.set_agent_ready(text_runner.agent_id())?;
    if let Some(task_id) = handle.try_schedule_next()? {
        let task = handle.get_task(&task_id)?;
        let output = text_runner.run_task(task).await?;
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
