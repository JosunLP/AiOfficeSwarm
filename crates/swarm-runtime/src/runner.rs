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

use std::collections::HashSet;
use std::sync::Arc;

use swarm_core::{
    agent::Agent,
    error::{SwarmError, SwarmResult},
    event::EventKind,
    identity::AgentId,
    task::Task,
};
use swarm_learning::{
    output::LearningCategory, LearningContext, LearningEvent, LearningOutput, LearningScope,
    LearningStore, LearningStrategy,
};
use swarm_memory::{
    entry::{MemoryEntry, MemoryScope, MemoryType, SensitivityLevel},
    MemoryBackend, MemoryQuery,
};
use swarm_orchestrator::OrchestratorHandle;
use swarm_personality::{PersonalityId, PersonalityProfile, PersonalityRegistry};
use swarm_policy::PolicyEngine;
use swarm_provider::{
    ProviderCapabilities, ProviderRegistry, ProviderRouter, RoutingContext, RoutingStrategy,
    StrategyRouter,
};
use swarm_role::{personality_bridge, RoleRegistry, RoleSpec};

use crate::circuit_breaker::CircuitBreaker;

/// Optional runtime integrations that enrich task execution with enterprise
/// context such as policy checks, role-aware personality overlays, memory
/// retrieval, learning output capture, and provider routing hints.
#[derive(Clone, Default)]
pub struct TaskExecutionContext {
    policy_engine: Option<PolicyEngine>,
    role_registry: Option<RoleRegistry>,
    personality_registry: Option<Arc<PersonalityRegistry>>,
    default_personality: Option<PersonalityProfile>,
    memory_backend: Option<Arc<dyn MemoryBackend>>,
    learning_store: Option<Arc<dyn LearningStore>>,
    learning_strategies: Vec<Arc<dyn LearningStrategy>>,
    provider_registry: Option<Arc<ProviderRegistry>>,
    provider_routing_strategy: RoutingStrategy,
    learning_scope: Option<LearningScope>,
}

impl TaskExecutionContext {
    /// Create an empty execution context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a policy engine for execution-time governance checks.
    pub fn with_policy_engine(mut self, policy_engine: PolicyEngine) -> Self {
        self.policy_engine = Some(policy_engine);
        self
    }

    /// Attach a role registry for role resolution during task execution.
    pub fn with_role_registry(mut self, role_registry: RoleRegistry) -> Self {
        self.role_registry = Some(role_registry);
        self
    }

    /// Attach a personality registry for direct personality lookup.
    pub fn with_personality_registry(
        mut self,
        personality_registry: Arc<PersonalityRegistry>,
    ) -> Self {
        self.personality_registry = Some(personality_registry);
        self
    }

    /// Set a default base personality used when an agent or role does not
    /// resolve to a registered profile.
    pub fn with_default_personality(mut self, personality: PersonalityProfile) -> Self {
        self.default_personality = Some(personality);
        self
    }

    /// Attach a memory backend for retrieval and episodic persistence.
    pub fn with_memory_backend(mut self, backend: Arc<dyn MemoryBackend>) -> Self {
        self.memory_backend = Some(backend);
        self
    }

    /// Attach a learning store for post-execution learning capture.
    pub fn with_learning_store(mut self, store: Arc<dyn LearningStore>) -> Self {
        self.learning_store = Some(store);
        self
    }

    /// Attach a learning strategy for structured post-execution observations.
    pub fn with_learning_strategy(mut self, strategy: Arc<dyn LearningStrategy>) -> Self {
        self.learning_strategies.push(strategy);
        self
    }

    /// Attach a provider registry for capability-aware routing hints.
    pub fn with_provider_registry(mut self, registry: Arc<ProviderRegistry>) -> Self {
        self.provider_registry = Some(registry);
        self
    }

    /// Select the built-in provider routing strategy.
    pub fn with_provider_routing_strategy(mut self, strategy: RoutingStrategy) -> Self {
        self.provider_routing_strategy = strategy;
        self
    }

    /// Set the scope used for learning output persistence.
    pub fn with_learning_scope(mut self, scope: LearningScope) -> Self {
        self.learning_scope = Some(scope);
        self
    }
}

#[derive(Debug, Clone)]
struct TaskExecutionSnapshot {
    task_id: swarm_core::identity::TaskId,
    task_name: String,
    input: serde_json::Value,
    metadata: serde_json::Value,
    required_capabilities: Vec<String>,
}

impl TaskExecutionSnapshot {
    fn from_task(task: &Task) -> Self {
        let mut required_capabilities = task
            .spec
            .required_capabilities
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        required_capabilities.sort();

        Self {
            task_id: task.id,
            task_name: task.spec.name.clone(),
            input: task.spec.input.clone(),
            metadata: serde_json::to_value(&task.spec.metadata.0)
                .unwrap_or(serde_json::Value::Null),
            required_capabilities,
        }
    }
}

/// Drives a single agent through task execution.
pub struct TaskRunner {
    agent: Box<dyn Agent>,
    handle: OrchestratorHandle,
    circuit_breaker: CircuitBreaker,
    execution_context: TaskExecutionContext,
}

impl TaskRunner {
    /// Create a new task runner for the given agent.
    pub fn new(agent: Box<dyn Agent>, handle: OrchestratorHandle) -> Self {
        let name = agent.descriptor().name.clone();
        Self {
            agent,
            handle,
            circuit_breaker: CircuitBreaker::new(name),
            execution_context: TaskExecutionContext::default(),
        }
    }

    /// Attach optional runtime integrations to this task runner.
    pub fn with_execution_context(mut self, execution_context: TaskExecutionContext) -> Self {
        self.execution_context = execution_context;
        self
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

        if let Err(error) = self.enforce_execution_policy(task_id).await {
            self.circuit_breaker.record_failure();
            self.handle
                .record_task_failed(task_id, agent_id, error.to_string())?;
            return Err(error);
        }

        let task = match self.enrich_task(task).await {
            Ok(task) => task,
            Err(error) => {
                self.circuit_breaker.record_failure();
                self.handle
                    .record_task_failed(task_id, agent_id, error.to_string())?;
                return Err(error);
            }
        };
        let snapshot = TaskExecutionSnapshot::from_task(&task);

        // Tell the orchestrator execution is starting.
        // If this fails (e.g., wrong assigned agent), record the task as
        // failed and reset the agent so neither gets stuck.
        if let Err(start_err) = self.handle.record_task_started(task_id, agent_id) {
            if let Err(record_err) =
                self.handle
                    .record_task_failed(task_id, agent_id, start_err.to_string())
            {
                tracing::error!(
                    task_id = %task_id,
                    agent_id = %agent_id,
                    error = %record_err,
                    "failed to record task failure after record_task_started error"
                );
            }
            return Err(start_err);
        }

        match self.effective_timeout(&task) {
            Some(timeout) => {
                self.execute_with_timeout(task_id, agent_id, task, snapshot, timeout)
                    .await
            }
            None => self.execute_without_timeout(agent_id, task, snapshot).await,
        }
    }

    async fn execute_with_timeout(
        &mut self,
        task_id: swarm_core::identity::TaskId,
        agent_id: AgentId,
        task: Task,
        snapshot: TaskExecutionSnapshot,
        timeout: std::time::Duration,
    ) -> SwarmResult<serde_json::Value> {
        match tokio::time::timeout(timeout, self.agent.execute(task)).await {
            Ok(result) => self.finish_execution(agent_id, snapshot, result).await,
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

    async fn execute_without_timeout(
        &mut self,
        agent_id: AgentId,
        task: Task,
        snapshot: TaskExecutionSnapshot,
    ) -> SwarmResult<serde_json::Value> {
        let result = self.agent.execute(task).await;
        self.finish_execution(agent_id, snapshot, result).await
    }

    fn effective_timeout(&self, task: &Task) -> Option<std::time::Duration> {
        let agent_limit = self.agent.descriptor().resource_limits.max_execution_time;
        match (task.spec.timeout, agent_limit) {
            (Some(task_timeout), Some(agent_timeout)) => Some(task_timeout.min(agent_timeout)),
            (Some(task_timeout), None) => Some(task_timeout),
            (None, Some(agent_timeout)) => Some(agent_timeout),
            (None, None) => None,
        }
    }

    async fn finish_execution(
        &mut self,
        agent_id: AgentId,
        snapshot: TaskExecutionSnapshot,
        result: SwarmResult<serde_json::Value>,
    ) -> SwarmResult<serde_json::Value> {
        match result {
            Ok(output) => {
                self.circuit_breaker.record_success();
                self.handle
                    .record_task_completed(snapshot.task_id, agent_id, output.clone())?;
                self.persist_post_execution_artifacts(&snapshot, &output, None)
                    .await?;
                Ok(output)
            }
            Err(e) => {
                self.circuit_breaker.record_failure();
                self.handle
                    .record_task_failed(snapshot.task_id, agent_id, e.to_string())?;
                self.persist_post_execution_artifacts(
                    &snapshot,
                    &serde_json::Value::Null,
                    Some(&e),
                )
                .await?;
                Err(e)
            }
        }
    }

    async fn enforce_execution_policy(
        &self,
        task_id: swarm_core::identity::TaskId,
    ) -> SwarmResult<()> {
        let Some(policy_engine) = &self.execution_context.policy_engine else {
            return Ok(());
        };

        let descriptor = self.agent.descriptor();
        let mut context = swarm_core::policy::PolicyContext::new(
            "execute_task",
            descriptor.id.to_string(),
            task_id.to_string(),
        );
        context.attributes = serde_json::json!({
            "agent_name": descriptor.name,
            "agent_kind": descriptor.kind.label(),
            "role_id": descriptor.role_id,
            "trust_level": descriptor.trust_level as u8,
        });

        policy_engine.enforce(&context).await?;
        self.handle.publish_event(EventKind::PolicyEvaluated {
            action: context.action,
            subject: context.subject,
            decision: "allowed".into(),
        });
        Ok(())
    }

    async fn enrich_task(&self, task: Task) -> SwarmResult<Task> {
        let mut task = task;
        let descriptor = self.agent.descriptor();
        let role_spec = self.resolve_role_spec();

        if let Some(role_spec) = role_spec.as_ref() {
            task.spec
                .metadata
                .insert("swarm.role.name", role_spec.name.clone());
            task.spec
                .metadata
                .insert("swarm.role.department", role_spec.department.label());
            self.handle.publish_event(EventKind::RoleAssigned {
                agent_id: descriptor.id,
                role_name: role_spec.name.clone(),
            });
        }

        self.apply_personality_context(&mut task, role_spec.as_ref())?;
        self.apply_provider_context(&mut task).await?;
        self.apply_memory_context(&mut task, role_spec.as_ref())
            .await?;

        Ok(task)
    }

    fn resolve_role_spec(&self) -> Option<RoleSpec> {
        let role_name = self.agent.descriptor().role_id.as_deref()?;
        let registry = self.execution_context.role_registry.as_ref()?;
        registry.get_by_name(role_name).ok()
    }

    fn apply_personality_context(
        &self,
        task: &mut Task,
        role_spec: Option<&RoleSpec>,
    ) -> SwarmResult<()> {
        let descriptor = self.agent.descriptor();
        let mut personality = self.resolve_base_personality()?;

        if let Some(role_spec) = role_spec {
            let overlay = personality_bridge::overlay_from_role(role_spec);
            personality = personality.merge_overlay(&overlay);
            self.handle
                .publish_event(EventKind::PersonalityOverlayApplied {
                    agent_id: descriptor.id,
                    task_id: task.id,
                });
        }

        task.spec
            .metadata
            .insert("swarm.personality.name", personality.name.clone());
        task.spec.metadata.insert(
            "swarm.personality.tone",
            personality.communication_style.tone.clone(),
        );
        task.spec.metadata.insert(
            "swarm.personality.verbosity",
            personality.communication_style.verbosity.to_string(),
        );
        task.spec.metadata.insert(
            "swarm.personality.profile_json",
            serde_json::to_string(&personality).map_err(SwarmError::Serialization)?,
        );
        self.handle.publish_event(EventKind::PersonalityApplied {
            agent_id: descriptor.id,
            personality_name: personality.name,
        });
        Ok(())
    }

    fn resolve_base_personality(&self) -> SwarmResult<PersonalityProfile> {
        let descriptor = self.agent.descriptor();

        if let (Some(id), Some(registry)) = (
            descriptor.personality_id.as_deref(),
            self.execution_context.personality_registry.as_ref(),
        ) {
            let personality_id =
                id.parse::<PersonalityId>()
                    .map_err(|error| SwarmError::ConfigInvalid {
                        key: "agent.personality_id".into(),
                        reason: format!("invalid personality_id '{id}': {error}"),
                    })?;
            if let Some(profile) = registry.get(&personality_id) {
                return Ok(profile);
            }
        }

        Ok(self
            .execution_context
            .default_personality
            .clone()
            .unwrap_or_else(|| PersonalityProfile::new("Default Runtime Personality", "1.0.0")))
    }

    async fn apply_provider_context(&self, task: &mut Task) -> SwarmResult<()> {
        let Some(registry) = self.execution_context.provider_registry.as_ref() else {
            return Ok(());
        };
        if registry.is_empty() {
            return Ok(());
        }

        let descriptor = self.agent.descriptor();
        let preferred_provider_name = task
            .spec
            .metadata
            .get("swarm.provider.preferred")
            .map(ToOwned::to_owned)
            .or_else(|| descriptor.provider_preferences.preferred_provider.clone());
        let preferred_model = task
            .spec
            .metadata
            .get("swarm.provider.model")
            .map(ToOwned::to_owned)
            .or_else(|| descriptor.provider_preferences.preferred_model.clone());
        let required_capabilities = self.required_provider_capabilities(task);

        let allowlist = descriptor
            .provider_preferences
            .allowlist
            .iter()
            .filter_map(|name| registry.id_by_name(name))
            .collect();
        let blocklist = descriptor
            .provider_preferences
            .blocklist
            .iter()
            .filter_map(|name| registry.id_by_name(name))
            .collect();

        let selected = preferred_provider_name
            .as_deref()
            .and_then(|provider_name| {
                registry
                    .get_by_name(provider_name)
                    .filter(|provider| provider.capabilities().satisfies(&required_capabilities))
            });

        let provider = if let Some(provider) = selected {
            provider
        } else {
            let router =
                StrategyRouter::new(self.execution_context.provider_routing_strategy, registry);
            let decision = router
                .route(&RoutingContext {
                    required_capabilities,
                    preferred_model: preferred_model.clone(),
                    allowlist,
                    blocklist,
                    fallback_allowed: true,
                    ..RoutingContext::default()
                })
                .await?;
            if let Some(fallback) = decision.fallbacks.first() {
                self.handle.publish_event(EventKind::ProviderFailover {
                    from_provider: decision.provider.name().to_string(),
                    to_provider: fallback.name().to_string(),
                });
            }
            decision.provider
        };

        task.spec
            .metadata
            .insert("swarm.provider.id", provider.id().to_string());
        task.spec
            .metadata
            .insert("swarm.provider.name", provider.name().to_string());
        if let Some(model) = preferred_model {
            task.spec.metadata.insert("swarm.provider.model", model);
        }
        Ok(())
    }

    fn required_provider_capabilities(&self, task: &Task) -> ProviderCapabilities {
        let mut capabilities = ProviderCapabilities::default();
        let Some(raw) = task.spec.metadata.get("swarm.provider.capabilities") else {
            return capabilities;
        };

        for capability in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            match capability {
                "chat_completion" => capabilities.chat_completion = true,
                "streaming" => capabilities.streaming = true,
                "tool_calling" => capabilities.tool_calling = true,
                "reasoning" => capabilities.reasoning = true,
                "embeddings" => capabilities.embeddings = true,
                "speech" => capabilities.speech = true,
                "multimodal" => capabilities.multimodal = true,
                "vision" => capabilities.vision = true,
                "json_mode" => capabilities.json_mode = true,
                _ => {}
            }
        }

        capabilities
    }

    async fn apply_memory_context(
        &self,
        task: &mut Task,
        role_spec: Option<&RoleSpec>,
    ) -> SwarmResult<()> {
        let Some(memory_backend) = self.execution_context.memory_backend.as_ref() else {
            return Ok(());
        };

        let scopes = self.readable_memory_scopes(task, role_spec);
        if scopes.is_empty() {
            return Ok(());
        }

        let max_sensitivity = self.max_memory_sensitivity(role_spec);
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        let mut primary_scope_label = None;

        for scope in scopes {
            primary_scope_label.get_or_insert_with(|| scope.label().to_string());
            let query = MemoryQuery::all()
                .with_scope(scope)
                .with_limit(5)
                .with_max_sensitivity(max_sensitivity);
            for entry in memory_backend.retrieve(&query).await? {
                if seen.insert(entry.id) {
                    entries.push(entry);
                }
            }
        }

        if entries.is_empty() {
            return Ok(());
        }

        task.spec
            .metadata
            .insert("swarm.memory.entry_count", entries.len().to_string());
        task.spec.metadata.insert(
            "swarm.memory.context_json",
            serde_json::to_string(&entries).map_err(SwarmError::Serialization)?,
        );
        self.handle.publish_event(EventKind::MemoryRetrieved {
            scope: primary_scope_label.unwrap_or_else(|| "mixed".into()),
            count: entries.len(),
        });
        Ok(())
    }

    fn readable_memory_scopes(
        &self,
        task: &Task,
        role_spec: Option<&RoleSpec>,
    ) -> Vec<MemoryScope> {
        let descriptor = self.agent.descriptor();
        let mut labels: Vec<&str> = descriptor
            .memory_profile
            .readable_scopes
            .iter()
            .map(String::as_str)
            .collect();

        if let Some(role_spec) = role_spec {
            labels.extend(
                role_spec
                    .memory_policy
                    .readable_scopes
                    .iter()
                    .map(String::as_str),
            );
            labels.push("agent");
        }

        self.resolve_memory_scopes(task, labels)
    }

    fn writable_memory_scopes(
        &self,
        task_id: swarm_core::identity::TaskId,
        role_spec: Option<&RoleSpec>,
    ) -> Vec<MemoryScope> {
        let descriptor = self.agent.descriptor();
        let mut labels: Vec<&str> = descriptor
            .memory_profile
            .writable_scopes
            .iter()
            .map(String::as_str)
            .collect();

        if let Some(role_spec) = role_spec {
            labels.extend(
                role_spec
                    .memory_policy
                    .writable_scopes
                    .iter()
                    .map(String::as_str),
            );
        }

        let synthetic_task = Task {
            id: task_id,
            spec: swarm_core::task::TaskSpec::new(
                "synthetic-memory-scope",
                serde_json::Value::Null,
            ),
            status: swarm_core::task::TaskStatus::Pending,
            created_at: swarm_core::types::now(),
            updated_at: swarm_core::types::now(),
            attempt_count: 0,
        };
        self.resolve_memory_scopes(&synthetic_task, labels)
    }

    fn resolve_memory_scopes(&self, task: &Task, labels: Vec<&str>) -> Vec<MemoryScope> {
        let descriptor = self.agent.descriptor();
        let mut scopes = Vec::new();
        let mut seen = HashSet::new();

        for label in labels {
            let scope =
                match label.to_ascii_lowercase().as_str() {
                    "task" => Some(MemoryScope::Task {
                        task_id: task.id.to_string(),
                    }),
                    "agent" | "own" => Some(MemoryScope::Agent {
                        agent_id: descriptor.id.to_string(),
                    }),
                    "session" => task.spec.metadata.get("session_id").map(|session_id| {
                        MemoryScope::Session {
                            session_id: session_id.to_string(),
                        }
                    }),
                    "team" | "shared" => {
                        task.spec
                            .metadata
                            .get("team_id")
                            .map(|team_id| MemoryScope::Team {
                                team_id: team_id.to_string(),
                            })
                    }
                    "tenant" => {
                        task.spec
                            .metadata
                            .get("tenant_id")
                            .map(|tenant_id| MemoryScope::Tenant {
                                tenant_id: tenant_id.to_string(),
                            })
                    }
                    "long_term" | "longterm" | "persistent" => Some(MemoryScope::LongTerm {
                        owner_id: descriptor.id.to_string(),
                    }),
                    _ => None,
                };

            if let Some(scope) = scope {
                let fingerprint = format!("{}:{:?}", scope.label(), scope);
                if seen.insert(fingerprint) {
                    scopes.push(scope);
                }
            }
        }

        scopes
    }

    fn max_memory_sensitivity(&self, role_spec: Option<&RoleSpec>) -> SensitivityLevel {
        let role_level = role_spec
            .and_then(|role_spec| role_spec.memory_policy.max_sensitivity.as_deref())
            .map(parse_sensitivity);
        let descriptor_level = self
            .agent
            .descriptor()
            .memory_profile
            .max_sensitivity
            .as_deref()
            .map(parse_sensitivity);

        match (role_level, descriptor_level) {
            (Some(role_level), Some(descriptor_level)) => role_level.min(descriptor_level),
            (Some(level), None) | (None, Some(level)) => level,
            (None, None) => SensitivityLevel::Internal,
        }
    }

    async fn persist_post_execution_artifacts(
        &self,
        snapshot: &TaskExecutionSnapshot,
        output: &serde_json::Value,
        error: Option<&SwarmError>,
    ) -> SwarmResult<()> {
        let role_spec = self.resolve_role_spec();
        if let Some(memory_backend) = self.execution_context.memory_backend.as_ref() {
            let scopes = self.writable_memory_scopes(snapshot.task_id, role_spec.as_ref());
            for scope in scopes {
                let mut entry = MemoryEntry::new(
                    scope.clone(),
                    MemoryType::Episodic,
                    serde_json::json!({
                        "task_id": snapshot.task_id.to_string(),
                        "task_name": snapshot.task_name,
                        "status": if error.is_some() { "failed" } else { "completed" },
                        "output": output,
                        "error": error.map(ToString::to_string),
                    }),
                );
                entry.tags = vec!["runtime".into(), "task-execution".into()];
                entry.sensitivity = self.max_memory_sensitivity(role_spec.as_ref());
                memory_backend.store(entry).await?;
                self.handle.publish_event(EventKind::MemoryStored {
                    scope: scope.label().into(),
                    memory_type: MemoryType::Episodic.label().into(),
                });
            }
        }

        if let Some(learning_store) = self.execution_context.learning_store.as_ref() {
            let descriptor = self.agent.descriptor();
            if descriptor.learning_policy.enabled {
                let requires_approval = descriptor.learning_policy.require_approval
                    || role_spec
                        .as_ref()
                        .map(|spec| spec.learning_policy.require_approval)
                        .unwrap_or(false);
                let category = if error.is_some() {
                    LearningCategory::FeedbackIncorporation
                } else {
                    LearningCategory::PatternExtraction
                };

                let baseline_output = if requires_approval {
                    LearningOutput::requires_review(
                        category.clone(),
                        format!("Execution outcome for task {}", snapshot.task_id),
                        serde_json::json!({
                            "task_id": snapshot.task_id.to_string(),
                            "task_name": snapshot.task_name,
                            "status": if error.is_some() { "failed" } else { "completed" },
                        }),
                        serde_json::json!({
                            "output": output,
                            "error": error.map(ToString::to_string),
                            "learning_scope": self
                                .execution_context
                                .learning_scope
                                .as_ref()
                                .map(LearningScope::label)
                                .unwrap_or("global"),
                        }),
                    )
                } else {
                    LearningOutput::auto(
                        category.clone(),
                        format!("Execution outcome for task {}", snapshot.task_id),
                        serde_json::json!({
                            "task_id": snapshot.task_id.to_string(),
                            "task_name": snapshot.task_name,
                            "status": if error.is_some() { "failed" } else { "completed" },
                        }),
                    )
                };
                self.record_learning_output(learning_store, role_spec.as_ref(), baseline_output)
                    .await?;

                if !self.execution_context.learning_strategies.is_empty() {
                    let event = self.learning_event(snapshot, output, error, role_spec.as_ref());
                    let context = self.learning_context(role_spec.as_ref());
                    for strategy in &self.execution_context.learning_strategies {
                        let strategy_requires_approval =
                            context.require_approval || strategy.always_requires_approval();
                        for learned_output in strategy.observe(&event, &context).await? {
                            if !self.learning_category_allowed(
                                &descriptor.learning_policy,
                                &learned_output,
                            ) {
                                continue;
                            }
                            let learned_output = self.normalized_learning_output(
                                role_spec.as_ref(),
                                learned_output,
                                strategy_requires_approval,
                            );
                            learning_store.record(learned_output.clone()).await?;
                            self.handle
                                .publish_event(EventKind::LearningOutputProduced {
                                    category: learning_category_label(&learned_output.category)
                                        .into(),
                                    requires_approval: learned_output.requires_approval,
                                });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn learning_context(&self, role_spec: Option<&RoleSpec>) -> LearningContext {
        LearningContext {
            scope: self
                .execution_context
                .learning_scope
                .clone()
                .unwrap_or_else(|| self.default_learning_scope(role_spec)),
            require_approval: self.agent.descriptor().learning_policy.require_approval
                || role_spec
                    .map(|spec| spec.learning_policy.require_approval)
                    .unwrap_or(false),
            tenant_id: self.learning_tenant_id(role_spec),
        }
    }

    fn default_learning_scope(&self, role_spec: Option<&RoleSpec>) -> LearningScope {
        if let Some(tenant_id) = self.learning_tenant_id(role_spec) {
            LearningScope::Tenant { tenant_id }
        } else {
            LearningScope::Agent {
                agent_id: self.agent.descriptor().id.to_string(),
            }
        }
    }

    fn learning_tenant_id(&self, role_spec: Option<&RoleSpec>) -> Option<String> {
        role_spec.and_then(|spec| {
            spec.metadata
                .custom
                .get("tenant_id")
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
        })
    }

    fn learning_event(
        &self,
        snapshot: &TaskExecutionSnapshot,
        output: &serde_json::Value,
        error: Option<&SwarmError>,
        role_spec: Option<&RoleSpec>,
    ) -> LearningEvent {
        LearningEvent {
            kind: if error.is_some() {
                swarm_learning::event::LearningEventKind::TaskFailed
            } else {
                swarm_learning::event::LearningEventKind::TaskCompleted
            },
            agent_id: self.agent.descriptor().id.to_string(),
            task_id: Some(snapshot.task_id.to_string()),
            tenant_id: self.learning_tenant_id(role_spec),
            timestamp: swarm_core::types::now(),
            payload: serde_json::json!({
                "task_name": snapshot.task_name,
                "input": snapshot.input,
                "output": output,
                "error": error.map(ToString::to_string),
                "required_capabilities": snapshot.required_capabilities,
                "metadata": snapshot.metadata,
            }),
        }
    }

    fn normalized_learning_output(
        &self,
        role_spec: Option<&RoleSpec>,
        mut output: LearningOutput,
        requires_approval: bool,
    ) -> LearningOutput {
        if let Some(scope) = self.execution_context.learning_scope.clone() {
            output.set_scope(scope);
        }
        output
            .agent_id
            .get_or_insert_with(|| self.agent.descriptor().id.to_string());
        if output.tenant_id.is_none() {
            output.tenant_id = self.learning_tenant_id(role_spec);
        }
        if matches!(output.scope, LearningScope::Global) {
            output.set_scope(self.default_learning_scope(role_spec));
        }
        if requires_approval {
            output.requires_approval = true;
            output.status = swarm_learning::LearningStatus::PendingApproval;
            output.applied_at = None;
        }
        output
    }

    async fn record_learning_output(
        &self,
        learning_store: &Arc<dyn LearningStore>,
        role_spec: Option<&RoleSpec>,
        output: LearningOutput,
    ) -> SwarmResult<()> {
        let output = self.normalized_learning_output(
            role_spec,
            output,
            self.agent.descriptor().learning_policy.require_approval
                || role_spec
                    .map(|spec| spec.learning_policy.require_approval)
                    .unwrap_or(false),
        );
        learning_store.record(output.clone()).await?;
        self.handle
            .publish_event(EventKind::LearningOutputProduced {
                category: learning_category_label(&output.category).into(),
                requires_approval: output.requires_approval,
            });
        Ok(())
    }

    fn learning_category_allowed(
        &self,
        policy: &swarm_core::agent::LearningPolicyRef,
        output: &LearningOutput,
    ) -> bool {
        policy.allowed_categories.is_empty()
            || policy
                .allowed_categories
                .iter()
                .any(|category| category == learning_category_label(&output.category))
    }
}

fn parse_sensitivity(label: &str) -> SensitivityLevel {
    match label.to_ascii_lowercase().as_str() {
        "public" => SensitivityLevel::Public,
        "internal" => SensitivityLevel::Internal,
        "confidential" => SensitivityLevel::Confidential,
        "restricted" => SensitivityLevel::Restricted,
        _ => SensitivityLevel::Internal,
    }
}

fn learning_category_label(category: &LearningCategory) -> &str {
    match category {
        LearningCategory::PreferenceAdaptation => "preference_adaptation",
        LearningCategory::PatternExtraction => "pattern_extraction",
        LearningCategory::FeedbackIncorporation => "feedback_incorporation",
        LearningCategory::PlanTemplate => "plan_template",
        LearningCategory::ScoringImprovement => "scoring_improvement",
        LearningCategory::KnowledgeAccumulation => "knowledge_accumulation",
        LearningCategory::ConfigurationEvolution => "configuration_evolution",
        LearningCategory::FineTuningData => "fine_tuning_data",
        LearningCategory::Custom(value) => value.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::{sync::Arc, time::Duration};
    use swarm_core::{
        agent::{AgentDescriptor, AgentKind},
        capability::CapabilitySet,
        task::TaskSpec,
        ResourceLimits,
    };
    use swarm_learning::{store::InMemoryLearningStore, ExecutionTemplateStrategy, LearningStatus};
    use swarm_memory::{in_memory::InMemoryBackend, MemoryBackend};
    use swarm_orchestrator::Orchestrator;
    use swarm_personality::PersonalityProfile;
    use swarm_policy::{AllowAllPolicy, DenyAllPolicy, PolicyEngine};
    use swarm_provider::{ModelProvider, ProviderCapabilities};
    use swarm_role::{model::DepartmentCategory, RoleRegistry, RoleSpec};

    struct OkAgent {
        descriptor: AgentDescriptor,
    }
    struct FailAgent {
        descriptor: AgentDescriptor,
    }
    struct SlowAgent {
        descriptor: AgentDescriptor,
    }
    struct VerySlowAgent {
        descriptor: AgentDescriptor,
    }
    struct ContextAwareAgent {
        descriptor: AgentDescriptor,
    }
    struct TestProvider {
        id: swarm_core::PluginId,
    }

    const VERY_SLOW_TASK_DURATION: Duration = Duration::from_secs(500);

    #[async_trait]
    impl Agent for OkAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }
        async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
            Ok(task.spec.input.clone())
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Agent for FailAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }
        async fn execute(&mut self, _task: Task) -> SwarmResult<serde_json::Value> {
            Err(SwarmError::Internal {
                reason: "agent failed".into(),
            })
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Agent for SlowAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }
        async fn execute(&mut self, _task: Task) -> SwarmResult<serde_json::Value> {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(serde_json::json!({"slow": true}))
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Agent for VerySlowAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }
        async fn execute(&mut self, _task: Task) -> SwarmResult<serde_json::Value> {
            tokio::time::sleep(VERY_SLOW_TASK_DURATION).await;
            Ok(serde_json::json!({"slow": true}))
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Agent for ContextAwareAgent {
        fn descriptor(&self) -> &AgentDescriptor {
            &self.descriptor
        }

        async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
            Ok(serde_json::json!({
                "provider": task.spec.metadata.get("swarm.provider.name"),
                "personality": task.spec.metadata.get("swarm.personality.name"),
                "memory_entries": task.spec.metadata.get("swarm.memory.entry_count"),
                "role": task.spec.metadata.get("swarm.role.name"),
            }))
        }

        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ModelProvider for TestProvider {
        fn id(&self) -> swarm_core::PluginId {
            self.id
        }

        fn name(&self) -> &str {
            "test-provider"
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                chat_completion: true,
                ..Default::default()
            }
        }

        async fn chat_completion(
            &self,
            request: swarm_provider::ChatRequest,
        ) -> SwarmResult<swarm_provider::ChatResponse> {
            Ok(swarm_provider::ChatResponse {
                model: request.model,
                content: Some("ok".into()),
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
                latency_ms: Some(10),
                message: None,
            })
        }
    }

    #[tokio::test]
    async fn run_task_success() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("ok-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({"x": 1})))
            .unwrap();
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
        let agent = FailAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({})))
            .unwrap();
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
        let agent = SlowAgent {
            descriptor: desc.clone(),
        };
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

    #[tokio::test(start_paused = true)]
    async fn run_task_without_timeout_and_disabled_defaults_keeps_running() {
        let orch = Orchestrator::with_config(swarm_orchestrator::OrchestratorConfig {
            default_task_timeout: None,
            ..swarm_orchestrator::OrchestratorConfig::default()
        });
        let handle = orch.handle();

        let mut desc =
            AgentDescriptor::new("very-slow-worker", AgentKind::Worker, CapabilitySet::new());
        desc.resource_limits = ResourceLimits::unlimited();
        let agent = VerySlowAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let mut spec = TaskSpec::new("t", serde_json::json!({}));
        spec.timeout = None;
        let task_id = handle.submit_task(spec).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone());
        let run = tokio::spawn(async move { runner.run_task(task).await });

        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(301)).await;
        tokio::task::yield_now().await;

        assert!(
            !run.is_finished(),
            "task with timeout=None should keep running when orchestrator and agent defaults are disabled"
        );

        let running_task = handle.get_task(&task_id).unwrap();
        assert_eq!(running_task.status.label(), "running");

        run.abort();
    }

    #[tokio::test]
    async fn run_task_without_task_timeout_uses_agent_execution_limit() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let mut desc =
            AgentDescriptor::new("limited-worker", AgentKind::Worker, CapabilitySet::new());
        desc.resource_limits.max_execution_time = Some(Duration::from_millis(5));
        let agent = SlowAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let mut spec = TaskSpec::new("t", serde_json::json!({}));
        spec.timeout = None;
        let task_id = handle.submit_task(spec).unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone());

        assert!(matches!(
            runner.run_task(task).await,
            Err(SwarmError::TaskTimeout { id, .. }) if id == task_id
        ));
    }

    #[tokio::test]
    async fn run_task_preserves_circuit_breaker_error_details() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let desc = AgentDescriptor::new("ok-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({"x": 1})))
            .unwrap();
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

    #[tokio::test]
    async fn task_runner_enriches_execution_context_and_persists_learning() {
        let orch = Orchestrator::new();
        let handle = orch.handle();

        let memory_backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let learning_store = Arc::new(InMemoryLearningStore::new());
        let provider_registry = Arc::new(ProviderRegistry::new());
        provider_registry
            .register(Arc::new(TestProvider {
                id: swarm_core::PluginId::new(),
            }))
            .unwrap();

        let role_registry = RoleRegistry::new();
        let mut role = RoleSpec::new("Support Agent", DepartmentCategory::Customer);
        role.memory_policy.readable_scopes = vec!["agent".into()];
        role.memory_policy.writable_scopes = vec!["agent".into()];
        role.learning_policy.enabled = true;
        role.learning_policy.require_approval = true;
        role.personality.tone = Some("empathetic".into());
        role_registry.register(role).unwrap();

        let mut memory_entry = MemoryEntry::new(
            MemoryScope::Agent {
                agent_id: "agent-placeholder".into(),
            },
            MemoryType::Summary,
            serde_json::json!({"summary": "recent context"}),
        );

        let mut descriptor =
            AgentDescriptor::new("context-worker", AgentKind::Worker, CapabilitySet::new());
        descriptor.role_id = Some("Support Agent".into());
        descriptor.learning_policy.enabled = true;
        descriptor.learning_policy.require_approval = true;
        descriptor.provider_preferences.preferred_provider = Some("test-provider".into());
        let agent_id = descriptor.id;
        memory_entry.scope = MemoryScope::Agent {
            agent_id: agent_id.to_string(),
        };
        memory_backend.store(memory_entry).await.unwrap();

        let agent = ContextAwareAgent {
            descriptor: descriptor.clone(),
        };

        handle.register_agent(descriptor).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({"x": 1})))
            .unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone()).with_execution_context(
            TaskExecutionContext::new()
                .with_role_registry(role_registry)
                .with_memory_backend(Arc::clone(&memory_backend))
                .with_learning_store(learning_store.clone())
                .with_learning_strategy(Arc::new(ExecutionTemplateStrategy::new()))
                .with_learning_scope(LearningScope::Team {
                    team_id: "support-ops".into(),
                })
                .with_provider_registry(provider_registry)
                .with_default_personality(PersonalityProfile::new("Base", "1.0.0")),
        );

        let output = runner.run_task(task).await.unwrap();

        assert_eq!(output["provider"], "test-provider");
        assert_eq!(output["role"], "Support Agent");
        assert_eq!(output["personality"], "Base");
        assert_eq!(output["memory_entries"], "1");

        let stored_entries = memory_backend
            .retrieve(&MemoryQuery::all().with_scope(MemoryScope::Agent {
                agent_id: agent_id.to_string(),
            }))
            .await
            .unwrap();
        assert!(stored_entries.len() >= 2);

        let pending = learning_store
            .list_pending_approvals(&LearningScope::Team {
                team_id: "support-ops".into(),
            })
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .all(|output| output.status == LearningStatus::PendingApproval));
        assert!(pending.iter().all(|output| {
            output.scope
                == LearningScope::Team {
                    team_id: "support-ops".into(),
                }
        }));
        assert!(pending.iter().any(|output| {
            output.category == LearningCategory::PlanTemplate
                && output.delta["template_name"] == serde_json::json!("t")
        }));
    }

    #[tokio::test]
    async fn task_runner_honors_execution_policy() {
        let orch = Orchestrator::new();
        let handle = orch.handle();
        let policy_engine = PolicyEngine::deny_by_default();
        policy_engine
            .register(Arc::new(DenyAllPolicy::new("deny-exec", "blocked")))
            .await;

        let desc = AgentDescriptor::new("guarded-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({})))
            .unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle.clone())
            .with_execution_context(TaskExecutionContext::new().with_policy_engine(policy_engine));

        assert!(matches!(
            runner.run_task(task).await,
            Err(SwarmError::PolicyViolation { .. })
        ));

        let failed_task = handle.get_task(&task_id).unwrap();
        assert_eq!(failed_task.status.label(), "failed");
    }

    #[tokio::test]
    async fn task_runner_emits_policy_evaluated_event_on_allow() {
        let orch = Orchestrator::new();
        let mut rx = orch.subscribe();
        let handle = orch.handle();
        let policy_engine = PolicyEngine::deny_by_default();
        policy_engine
            .register(Arc::new(AllowAllPolicy::new("allow-exec")))
            .await;

        let desc = AgentDescriptor::new("ok-worker", AgentKind::Worker, CapabilitySet::new());
        let agent = OkAgent {
            descriptor: desc.clone(),
        };
        let agent_id = desc.id;

        handle.register_agent(desc).unwrap();
        handle.set_agent_ready(agent_id).unwrap();

        let task_id = handle
            .submit_task(TaskSpec::new("t", serde_json::json!({"ok": true})))
            .unwrap();
        handle.try_schedule_next().unwrap();

        let task = handle.get_task(&task_id).unwrap();
        let mut runner = TaskRunner::new(Box::new(agent), handle)
            .with_execution_context(TaskExecutionContext::new().with_policy_engine(policy_engine));

        runner.run_task(task).await.unwrap();

        loop {
            let event = rx.try_recv().unwrap();
            if let EventKind::PolicyEvaluated {
                action, decision, ..
            } = event.kind
            {
                assert_eq!(action, "execute_task");
                assert_eq!(decision, "allowed");
                break;
            }
        }
    }
}
