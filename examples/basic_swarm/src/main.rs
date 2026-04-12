//! # Basic Swarm Example
//!
//! This example demonstrates an end-to-end AiOfficeSwarm baseline with:
//!
//! 1. telemetry and orchestrator setup,
//! 2. role loading,
//! 3. provider registration,
//! 4. memory seeding,
//! 5. runtime context assembly for policy, roles, memory, learning, and providers,
//! 6. agent execution,
//! 7. plugin invocation.

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
use swarm_learning::{
    output::LearningCategory, ExecutionTemplateStrategy, FileLearningStore, LearningScope,
    LearningStore,
};
use swarm_memory::{
    in_memory::InMemoryBackend, MemoryBackend, MemoryEntry, MemoryScope, MemoryType,
};
use swarm_orchestrator::Orchestrator;
use swarm_personality::PersonalityProfile;
use swarm_plugin::PluginHost;
use swarm_policy::{AllowAllPolicy, PolicyEngine};
use swarm_provider::{
    ChatRequest, ChatResponse, ModelProvider, ProviderCapabilities, ProviderRegistry,
};
use swarm_role::RoleLoadOptions;
use swarm_runtime::{TaskExecutionContext, TaskRunner};
use swarm_telemetry::{init_tracing, Metrics};

fn orchestrator_config_from_swarm(
    config: &swarm_config::model::OrchestratorConfig,
) -> swarm_orchestrator::OrchestratorConfig {
    swarm_orchestrator::OrchestratorConfig {
        event_channel_capacity: config.event_channel_capacity,
        max_dispatch_per_tick: config.max_dispatch_per_tick,
        default_task_timeout: (config.default_task_timeout_secs != 0)
            .then(|| std::time::Duration::from_secs(config.default_task_timeout_secs)),
        max_concurrent_tasks: config.max_concurrent_tasks,
    }
}

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
        let word_count = text.split_whitespace().count();
        info!(
            agent = %self.descriptor.name,
            task = task.spec.name,
            word_count,
            provider = task.spec.metadata.get("swarm.provider.name").unwrap_or("n/a"),
            personality = task.spec.metadata.get("swarm.personality.name").unwrap_or("n/a"),
            "Processing text task"
        );
        Ok(json!({
            "agent": &self.descriptor.name,
            "original_length": text.len(),
            "word_count": word_count,
            "summary": format!("[Summary of {} words]", word_count),
            "provider": task.spec.metadata.get("swarm.provider.name"),
            "personality": task.spec.metadata.get("swarm.personality.name"),
            "memory_entries": task.spec.metadata.get("swarm.memory.entry_count"),
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

/// Minimal built-in provider used only to demonstrate provider-aware runtime
/// selection metadata in the example.
struct DemoProvider {
    id: swarm_core::PluginId,
}

fn summarize_values(values: &[f64]) -> (f64, f64, Option<f64>, Option<f64>) {
    let sum: f64 = values.iter().sum();
    let mean = if values.is_empty() {
        0.0
    } else {
        sum / values.len() as f64
    };
    let max = values.iter().copied().reduce(f64::max);
    let min = values.iter().copied().reduce(f64::min);
    (sum, mean, max, min)
}

async fn enforce_policy_check(
    policy_engine: &PolicyEngine,
    action: &str,
    resource: impl Into<String>,
) -> SwarmResult<()> {
    policy_engine
        .enforce(&PolicyContext::new(action, "basic_swarm", resource.into()))
        .await
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
            provider = task.spec.metadata.get("swarm.provider.name").unwrap_or("n/a"),
            "Analyzing data"
        );

        Ok(json!({
            "agent": &self.descriptor.name,
            "n": values.len(),
            "sum": sum,
            "mean": mean,
            "max": max,
            "min": min,
            "provider": task.spec.metadata.get("swarm.provider.name"),
            "personality": task.spec.metadata.get("swarm.personality.name"),
            "memory_entries": task.spec.metadata.get("swarm.memory.entry_count"),
        }))
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

#[async_trait]
impl ModelProvider for DemoProvider {
    fn id(&self) -> swarm_core::PluginId {
        self.id
    }

    fn name(&self) -> &str {
        "demo-provider"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completion: true,
            json_mode: true,
            models: vec![swarm_provider::capabilities::ModelDescriptor {
                model_id: "demo-chat-v1".into(),
                display_name: "Demo Chat v1".into(),
                max_context_tokens: Some(8192),
                max_output_tokens: Some(1024),
                supports_tools: false,
                supports_vision: false,
                supports_streaming: false,
                supports_json_mode: true,
                is_reasoning_model: false,
            }],
            ..Default::default()
        }
    }

    async fn chat_completion(&self, request: ChatRequest) -> SwarmResult<ChatResponse> {
        Ok(ChatResponse {
            model: request.model,
            content: Some("demo".into()),
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
            response_id: None,
            extra: serde_json::Value::Null,
        })
    }

    async fn health_check(&self) -> SwarmResult<swarm_provider::traits::ProviderHealth> {
        Ok(swarm_provider::traits::ProviderHealth {
            healthy: true,
            latency_ms: Some(5),
            message: Some("demo provider healthy".into()),
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ConfigLoader::with_env_overrides(ConfigLoader::defaults());
    init_tracing(&config.telemetry);

    println!("╔═══════════════════════════════════════════════╗");
    println!("║  AiOfficeSwarm — Runtime Context Demo        ║");
    println!("╚═══════════════════════════════════════════════╝\n");

    let orch = Orchestrator::with_config(orchestrator_config_from_swarm(&config.orchestrator));
    let handle = orch.handle();
    let metrics = Metrics::new();

    let policy_engine = PolicyEngine::allow_by_default();
    policy_engine
        .register(Arc::new(AllowAllPolicy::new("demo-allow-all")))
        .await;

    println!("── Loading Roles ───────────────────────────────");
    let role_registry = swarm_role::RoleRegistry::new();
    let roles_dir = std::path::Path::new("roles");
    if roles_dir.exists() {
        match swarm_role::RoleLoader::load_directory_with_options(
            roles_dir,
            &role_registry,
            RoleLoadOptions {
                treat_warnings_as_errors: config.roles.strict_validation,
            },
        ) {
            Ok(summary) => {
                println!(
                    "  ✓ Loaded {} / {} roles",
                    summary.loaded, summary.total_files
                );
                if summary.errors > 0 {
                    println!("  ⚠ {} files failed to load", summary.errors);
                }
                if summary.has_blocking_issues(RoleLoadOptions {
                    treat_warnings_as_errors: config.roles.strict_validation,
                }) {
                    return Err(anyhow::anyhow!(
                        "role loading produced blocking issues (strict_validation={})",
                        config.roles.strict_validation
                    ));
                }
            }
            Err(e) => println!("  ⚠ Could not load roles: {e}"),
        }
    } else {
        println!("  (roles/ directory not found, skipping)");
    }
    println!();

    let memory_backend = Arc::new(InMemoryBackend::new());
    let learning_store = Arc::new(FileLearningStore::new(&config.learning.store_path));
    let execution_template_strategy = Arc::new(ExecutionTemplateStrategy::new());
    let provider_registry = Arc::new(ProviderRegistry::new());
    provider_registry.register(Arc::new(DemoProvider {
        id: swarm_core::PluginId::new(),
    }))?;
    println!("  Learning store: {}", learning_store.path().display());

    println!("── Registering Agents ──────────────────────────");
    let mut text_agent = TextProcessingAgent::new("TextProcessor-1");
    text_agent.descriptor.role_id = Some("Support Agent".into());
    text_agent.descriptor.learning_policy.enabled = true;
    text_agent.descriptor.learning_policy.require_approval = true;
    text_agent.descriptor.memory_profile.readable_scopes = vec!["agent".into()];
    text_agent.descriptor.memory_profile.writable_scopes = vec!["agent".into()];
    text_agent
        .descriptor
        .provider_preferences
        .preferred_provider = Some("demo-provider".into());
    text_agent.descriptor.provider_preferences.preferred_model = Some("demo-chat-v1".into());
    let text_agent_id = handle.register_agent(text_agent.descriptor().clone())?;
    handle.set_agent_ready(text_agent_id)?;
    metrics.inc_agents_registered();
    println!("  ✓ TextProcessor-1 registered");

    let mut data_agent = DataAnalysisAgent::new("DataAnalyst-1");
    data_agent.descriptor.role_id = Some("Data Analytics Agent".into());
    data_agent.descriptor.learning_policy.enabled = true;
    data_agent.descriptor.learning_policy.require_approval = true;
    data_agent.descriptor.memory_profile.readable_scopes = vec!["agent".into()];
    data_agent.descriptor.memory_profile.writable_scopes = vec!["agent".into()];
    data_agent
        .descriptor
        .provider_preferences
        .preferred_provider = Some("demo-provider".into());
    data_agent.descriptor.provider_preferences.preferred_model = Some("demo-chat-v1".into());
    let data_agent_id = handle.register_agent(data_agent.descriptor().clone())?;
    handle.set_agent_ready(data_agent_id)?;
    metrics.inc_agents_registered();
    println!("  ✓ DataAnalyst-1 registered");
    println!();

    memory_backend
        .store(MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: text_agent_id.to_string(),
            },
            MemoryType::Summary,
            json!({"summary": "Use concise, customer-safe language for summaries."}),
        ))
        .await?;
    memory_backend
        .store(MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: data_agent_id.to_string(),
            },
            MemoryType::Summary,
            json!({"summary": "Include mean, min, and max in analytics outputs."}),
        ))
        .await?;

    let execution_context = TaskExecutionContext::new()
        .with_policy_engine(policy_engine.clone())
        .with_role_registry(role_registry.clone())
        .with_memory_backend(memory_backend.clone())
        .with_learning_store(learning_store.clone())
        .with_learning_strategy(execution_template_strategy)
        .with_learning_scope(LearningScope::Global)
        .with_provider_registry(provider_registry.clone())
        .with_default_personality(PersonalityProfile::new("Enterprise Base", "1.0.0"));

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
    enforce_policy_check(&policy_engine, "submit_task", "task-queue").await?;
    handle.submit_task(text_spec)?;
    metrics.inc_tasks_submitted();

    let mut data_spec = TaskSpec::new(
        "analyze-sales",
        json!({ "values": [1200.5, 1350.0, 980.25, 1450.75, 1100.0, 1600.0] }),
    );
    data_spec.required_capabilities = {
        let mut c = CapabilitySet::new();
        c.add(Capability::new("data-analysis"));
        c
    };
    enforce_policy_check(&policy_engine, "submit_task", "task-queue").await?;
    handle.submit_task(data_spec)?;
    metrics.inc_tasks_submitted();

    enforce_policy_check(&policy_engine, "submit_task", "task-queue").await?;
    handle.submit_task(TaskSpec::new(
        "summarize-meeting-notes",
        json!({ "text": "Team standup: sprint velocity is on track, two blockers identified in the backend integration module." }),
    ))?;
    metrics.inc_tasks_submitted();
    println!("  ✓ Submitted three demo tasks");
    println!();

    println!("── Executing Tasks ─────────────────────────────");
    let mut text_runner = TaskRunner::new(Box::new(text_agent), handle.clone())
        .with_execution_context(execution_context.clone());
    let mut data_runner = TaskRunner::new(Box::new(data_agent), handle.clone())
        .with_execution_context(execution_context.clone());

    if let Some(task_id) = handle.try_schedule_next()? {
        let task = handle.get_task(&task_id)?;
        let output = text_runner.run_task(task).await?;
        metrics.inc_tasks_completed();
        println!(
            "  ✓ summarize-report → provider={}, personality={}, memory_entries={}",
            output["provider"].as_str().unwrap_or("n/a"),
            output["personality"].as_str().unwrap_or("n/a"),
            output["memory_entries"].as_str().unwrap_or("0")
        );
    }

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
        println!(
            "  ✓ analyze-sales    → mean={mean:.2}, max={max:.2}, provider={}, memory_entries={}",
            output["provider"].as_str().unwrap_or("n/a"),
            output["memory_entries"].as_str().unwrap_or("0")
        );
    }

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
        println!(
            "  ✓ summarize-meeting → summary={:?}, provider={}",
            output["summary"],
            output["provider"].as_str().unwrap_or("n/a")
        );
    }
    println!();

    println!("── Plugin Demonstration ────────────────────────");
    let plugin_host = PluginHost::new();
    let plugin_id = plugin_host
        .load(Box::new(example_integration::NotificationPlugin::new(
            "#ops-alerts",
        )))
        .await?;

    enforce_policy_check(&policy_engine, "invoke_plugin", plugin_id.to_string()).await?;
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

    println!("── Summary ─────────────────────────────────────");
    let snap = metrics.snapshot();
    println!("  Tasks submitted:        {}", snap.tasks_submitted);
    println!("  Tasks completed:        {}", snap.tasks_completed);
    println!("  Tasks failed:           {}", snap.tasks_failed);
    println!("  Agents active:          {}", snap.agents_registered);
    println!("  Plugin invocations:     {}", snap.plugin_invocations);
    let pending_learning = learning_store
        .list_pending_approvals(&LearningScope::Global)
        .await?;
    let learned_templates = learning_store
        .list(&LearningScope::Global)
        .await?
        .into_iter()
        .filter(|output| output.category == LearningCategory::PlanTemplate)
        .count();
    println!("  Pending learning queue: {}", pending_learning.len());
    println!("  Learned templates:      {}", learned_templates);

    println!("\nAll done! ✓");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{orchestrator_config_from_swarm, summarize_values};
    use swarm_config::SwarmConfig;

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

    #[test]
    fn example_uses_loaded_orchestrator_config() {
        let mut config = SwarmConfig::default();
        config.orchestrator.event_channel_capacity = 8;
        config.orchestrator.max_dispatch_per_tick = 2;
        config.orchestrator.default_task_timeout_secs = 12;
        config.orchestrator.max_concurrent_tasks = 5;

        let orchestrator_config = orchestrator_config_from_swarm(&config.orchestrator);

        assert_eq!(orchestrator_config.event_channel_capacity, 8);
        assert_eq!(orchestrator_config.max_dispatch_per_tick, 2);
        assert_eq!(
            orchestrator_config.default_task_timeout,
            Some(std::time::Duration::from_secs(12))
        );
        assert_eq!(orchestrator_config.max_concurrent_tasks, 5);
    }
}
