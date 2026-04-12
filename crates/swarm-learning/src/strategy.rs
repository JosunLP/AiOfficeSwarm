//! The core trait and built-in implementations for learning strategies.

use async_trait::async_trait;
use chrono::Utc;

use swarm_core::error::SwarmResult;

use crate::event::{LearningEvent, LearningEventKind};
use crate::output::{
    LearningCategory, LearningOutput, LearningResult, LearningRuleId, LearningStatus,
};
use crate::scope::LearningScope;

/// Context provided to a learning strategy during observation and application.
#[derive(Debug, Clone)]
pub struct LearningContext {
    /// The scope in which learning is occurring.
    pub scope: LearningScope,
    /// Whether human approval is required for this scope.
    pub require_approval: bool,
    /// The tenant this context belongs to (for isolation).
    pub tenant_id: Option<String>,
}

/// A learning strategy that observes events and produces learning outputs.
///
/// Implement this trait to create custom learning algorithms. Strategies are
/// registered with the learning subsystem and receive events relevant to
/// their scope.
#[async_trait]
pub trait LearningStrategy: Send + Sync {
    /// A unique identifier for this strategy.
    fn id(&self) -> LearningRuleId;

    /// Human-readable name for logging and audit.
    fn name(&self) -> &str;

    /// Observe a learning event and optionally produce outputs.
    ///
    /// The strategy may produce zero or more [`LearningOutput`] values
    /// from a single event.
    async fn observe(
        &self,
        event: &LearningEvent,
        ctx: &LearningContext,
    ) -> SwarmResult<Vec<LearningOutput>>;

    /// Apply a previously produced learning output.
    ///
    /// This is called only after the output has been approved (if approval
    /// is required). Returns a result describing whether application succeeded.
    async fn apply(
        &self,
        output: &LearningOutput,
        ctx: &LearningContext,
    ) -> SwarmResult<LearningResult>;

    /// Whether this strategy's outputs always require human approval,
    /// regardless of the scope configuration.
    fn always_requires_approval(&self) -> bool {
        false
    }
}

/// Built-in strategy that converts successful task executions into reusable
/// plan-template learning records.
#[derive(Debug, Clone)]
pub struct ExecutionTemplateStrategy {
    id: LearningRuleId,
    name: String,
}

impl Default for ExecutionTemplateStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionTemplateStrategy {
    /// Create the built-in execution-template strategy.
    pub fn new() -> Self {
        Self {
            id: LearningRuleId::new(),
            name: "execution-template".into(),
        }
    }
}

#[async_trait]
impl LearningStrategy for ExecutionTemplateStrategy {
    fn id(&self) -> LearningRuleId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn observe(
        &self,
        event: &LearningEvent,
        ctx: &LearningContext,
    ) -> SwarmResult<Vec<LearningOutput>> {
        if event.kind != LearningEventKind::TaskCompleted {
            return Ok(Vec::new());
        }

        let Some(task_name) = event
            .payload
            .get("task_name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(Vec::new());
        };

        let output_keys = sorted_object_keys(event.payload.get("output"));
        if output_keys.is_empty() {
            return Ok(Vec::new());
        }

        let input_keys = sorted_object_keys(event.payload.get("input"));
        let required_capabilities = sorted_string_array(event.payload.get("required_capabilities"));
        let provider = event
            .payload
            .get("metadata")
            .and_then(|metadata| metadata.get("swarm.provider.name"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let personality = event
            .payload
            .get("metadata")
            .and_then(|metadata| metadata.get("swarm.personality.name"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);

        let delta = serde_json::json!({
            "template_name": task_name,
            "source_task_id": event.task_id,
            "required_capabilities": required_capabilities,
            "input_keys": input_keys,
            "output_keys": output_keys,
            "provider": provider,
            "personality": personality,
        });
        let context = serde_json::json!({
            "strategy": self.name(),
            "event": event,
        });

        Ok(vec![LearningOutput {
            id: LearningRuleId::new(),
            category: LearningCategory::PlanTemplate,
            description: format!("Reusable execution template learned from task '{task_name}'"),
            scope: ctx.scope.clone(),
            agent_id: Some(event.agent_id.clone()),
            tenant_id: event.tenant_id.clone().or_else(|| ctx.tenant_id.clone()),
            context,
            delta,
            requires_approval: ctx.require_approval,
            status: if ctx.require_approval {
                LearningStatus::PendingApproval
            } else {
                LearningStatus::Pending
            },
            created_at: Utc::now(),
            applied_at: None,
        }])
    }

    async fn apply(
        &self,
        output: &LearningOutput,
        _ctx: &LearningContext,
    ) -> SwarmResult<LearningResult> {
        Ok(LearningResult {
            output_id: output.id,
            success: true,
            message: format!("{} accepted output {}", self.name(), output.id),
        })
    }

    fn always_requires_approval(&self) -> bool {
        true
    }
}

fn sorted_object_keys(value: Option<&serde_json::Value>) -> Vec<String> {
    let mut keys = value
        .and_then(serde_json::Value::as_object)
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn sorted_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    let mut items = value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    items.sort();
    // Sorting first ensures duplicate capability labels become adjacent so the
    // stored template delta remains deterministic after deduplication.
    items.dedup();
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execution_template_strategy_builds_plan_template_output() {
        let strategy = ExecutionTemplateStrategy::new();
        let event = LearningEvent::task_completed(
            "agent-1",
            "task-1",
            serde_json::json!({
                "task_name": "draft-plan",
                "input": { "ticket": 42, "priority": "high" },
                "output": { "summary": "done", "owner": "ops" },
                "required_capabilities": ["planning", "ticketing", "planning"],
                "metadata": {
                    "swarm.provider.name": "demo-provider",
                    "swarm.personality.name": "Enterprise Base"
                }
            }),
        );

        let outputs = strategy
            .observe(
                &event,
                &LearningContext {
                    scope: LearningScope::Workflow {
                        workflow_id: "triage".into(),
                    },
                    require_approval: false,
                    tenant_id: Some("tenant-1".into()),
                },
            )
            .await
            .unwrap();

        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].category, LearningCategory::PlanTemplate);
        assert_eq!(outputs[0].scope.label(), "workflow");
        assert_eq!(outputs[0].status, LearningStatus::Pending);
        assert_eq!(
            outputs[0].delta["required_capabilities"],
            serde_json::json!(["planning", "ticketing"])
        );
        assert_eq!(
            outputs[0].delta["output_keys"],
            serde_json::json!(["owner", "summary"])
        );
    }

    #[tokio::test]
    async fn execution_template_strategy_ignores_non_completed_events() {
        let strategy = ExecutionTemplateStrategy::new();
        let event = LearningEvent::feedback("agent-1", serde_json::json!({"score": 1}));

        let outputs = strategy
            .observe(
                &event,
                &LearningContext {
                    scope: LearningScope::Global,
                    require_approval: true,
                    tenant_id: None,
                },
            )
            .await
            .unwrap();

        assert!(outputs.is_empty());
    }
}
