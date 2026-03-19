//! Provider selection and routing strategies.
//!
//! The [`ProviderRouter`] trait defines how the framework selects a provider
//! for a given request. Implementations can consider capabilities, cost,
//! latency, compliance, and tenant preferences.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::{
    atomic::{AtomicUsize, Ordering as AtomicOrdering},
    Arc,
};

use swarm_core::error::{SwarmError, SwarmResult};
use swarm_core::identity::PluginId;

use crate::capabilities::ProviderCapabilities;
use crate::registry::ProviderRegistry;
use crate::traits::ModelProvider;

/// Cost optimization preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CostPreference {
    /// Prefer the cheapest provider.
    Cheapest,
    /// Balance cost and quality.
    #[default]
    Balanced,
    /// Prefer the highest-quality provider regardless of cost.
    BestQuality,
}

/// Latency optimization preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LatencyPreference {
    /// Prefer the fastest provider.
    Fastest,
    /// Balance latency and quality.
    #[default]
    Balanced,
    /// No latency preference.
    NoPreference,
}

/// Data locality requirement for compliance routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataLocality {
    /// Allowed regions (e.g., `"eu-west-1"`, `"us-east-1"`).
    pub allowed_regions: Vec<String>,
    /// Whether data may leave the specified regions.
    pub strict: bool,
}

/// Compliance requirement for provider routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceRequirement {
    /// The compliance standard (e.g., `"GDPR"`, `"HIPAA"`, `"SOC2"`).
    pub standard: String,
    /// Whether this requirement is mandatory or advisory.
    pub mandatory: bool,
}

/// Context for a provider routing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingContext {
    /// The capabilities the provider must support.
    pub required_capabilities: ProviderCapabilities,
    /// The specific model to use (if the caller has a model preference).
    pub preferred_model: Option<String>,
    /// Cost preference.
    pub cost_preference: CostPreference,
    /// Latency preference.
    pub latency_preference: LatencyPreference,
    /// Compliance requirements.
    pub compliance: Vec<ComplianceRequirement>,
    /// Data locality constraints.
    pub data_locality: Option<DataLocality>,
    /// Provider IDs explicitly allowed (empty = all allowed).
    pub allowlist: Vec<PluginId>,
    /// Provider IDs explicitly blocked.
    pub blocklist: Vec<PluginId>,
    /// Whether fallback to another provider is permitted on failure.
    pub fallback_allowed: bool,
}

impl Default for RoutingContext {
    fn default() -> Self {
        Self {
            required_capabilities: ProviderCapabilities::default(),
            preferred_model: None,
            cost_preference: CostPreference::default(),
            latency_preference: LatencyPreference::default(),
            compliance: Vec::new(),
            data_locality: None,
            allowlist: Vec::new(),
            blocklist: Vec::new(),
            fallback_allowed: true,
        }
    }
}

/// The result of a routing decision.
pub struct RoutingDecision {
    /// The selected provider.
    pub provider: Arc<dyn ModelProvider>,
    /// Ordered fallback providers (tried on failure if `fallback_allowed`).
    pub fallbacks: Vec<Arc<dyn ModelProvider>>,
}

impl std::fmt::Debug for RoutingDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingDecision")
            .field("provider", &self.provider.name())
            .field("fallbacks_count", &self.fallbacks.len())
            .finish()
    }
}

/// Strategy-based provider selection and failover.
///
/// Implement this trait to provide custom routing logic. The default
/// [`CapabilityMatchRouter`] selects the first provider that satisfies
/// the required capabilities.
#[async_trait]
pub trait ProviderRouter: Send + Sync {
    /// Select a provider (and optional fallbacks) for the given context.
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision>;
}

/// Predefined routing strategy types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingStrategy {
    /// Select the first provider that matches capabilities.
    #[default]
    CapabilityMatch,
    /// Select based on lowest cost estimate.
    LowestCost,
    /// Select based on lowest latency.
    LowestLatency,
    /// Round-robin across matching providers.
    RoundRobin,
}

fn provider_matches_model(
    provider: &Arc<dyn ModelProvider>,
    preferred_model: Option<&str>,
) -> bool {
    match preferred_model {
        None => true,
        Some(model) => provider
            .capabilities()
            .models
            .iter()
            .any(|descriptor| descriptor.model_id.eq_ignore_ascii_case(model)),
    }
}

fn numeric_hint(provider: &Arc<dyn ModelProvider>, key: &str) -> Option<f64> {
    provider
        .capabilities()
        .custom
        .get(key)
        .and_then(|value| match value {
            serde_json::Value::Number(number) => number.as_f64(),
            serde_json::Value::String(string) => string.parse::<f64>().ok(),
            _ => None,
        })
}

fn sort_by_numeric_hint(
    candidates: &mut [Arc<dyn ModelProvider>],
    preferred_model: Option<&str>,
    key: &str,
    descending: bool,
) {
    candidates.sort_by(|a, b| {
        let a_model = provider_matches_model(a, preferred_model);
        let b_model = provider_matches_model(b, preferred_model);
        b_model.cmp(&a_model).then_with(|| {
            let a_score = numeric_hint(a, key);
            let b_score = numeric_hint(b, key);
            let order = match (a_score, b_score) {
                (Some(a_score), Some(b_score)) => {
                    if descending {
                        b_score.partial_cmp(&a_score)
                    } else {
                        a_score.partial_cmp(&b_score)
                    }
                }
                (Some(_), None) => Some(Ordering::Less),
                (None, Some(_)) => Some(Ordering::Greater),
                (None, None) => Some(Ordering::Equal),
            }
            .unwrap_or(Ordering::Equal);

            order.then_with(|| a.name().cmp(b.name()))
        })
    });
}

fn finalize_decision(
    mut candidates: Vec<Arc<dyn ModelProvider>>,
    fallback_allowed: bool,
) -> SwarmResult<RoutingDecision> {
    if candidates.is_empty() {
        return Err(SwarmError::Internal {
            reason: "No provider matches the routing requirements".into(),
        });
    }

    let primary = candidates.remove(0);
    Ok(RoutingDecision {
        provider: primary,
        fallbacks: if fallback_allowed {
            candidates
        } else {
            Vec::new()
        },
    })
}

fn candidate_providers(
    registry: &ProviderRegistry,
    ctx: &RoutingContext,
) -> Vec<Arc<dyn ModelProvider>> {
    let mut candidates = registry.find_by_capabilities(&ctx.required_capabilities);

    if !ctx.allowlist.is_empty() {
        candidates.retain(|provider| ctx.allowlist.contains(&provider.id()));
    }
    if !ctx.blocklist.is_empty() {
        candidates.retain(|provider| !ctx.blocklist.contains(&provider.id()));
    }

    if let Some(preferred_model) = ctx.preferred_model.as_deref() {
        let preferred: Vec<_> = candidates
            .iter()
            .filter(|provider| provider_matches_model(provider, Some(preferred_model)))
            .cloned()
            .collect();
        if !preferred.is_empty() {
            return preferred;
        }
    }

    candidates
}

/// A simple router that selects providers based on capability matching.
pub struct CapabilityMatchRouter<'a> {
    registry: &'a ProviderRegistry,
}

impl<'a> CapabilityMatchRouter<'a> {
    /// Create a new capability-match router backed by the given registry.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ProviderRouter for CapabilityMatchRouter<'_> {
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision> {
        finalize_decision(
            candidate_providers(self.registry, ctx),
            ctx.fallback_allowed,
        )
    }
}

/// A router that selects the lowest-cost matching provider.
pub struct LowestCostRouter<'a> {
    registry: &'a ProviderRegistry,
}

impl<'a> LowestCostRouter<'a> {
    /// Create a new lowest-cost router backed by the given registry.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ProviderRouter for LowestCostRouter<'_> {
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision> {
        let mut candidates = candidate_providers(self.registry, ctx);
        sort_by_numeric_hint(
            &mut candidates,
            ctx.preferred_model.as_deref(),
            "estimated_cost_per_1k_tokens",
            false,
        );
        finalize_decision(candidates, ctx.fallback_allowed)
    }
}

/// A router that selects the lowest-latency matching provider.
pub struct LowestLatencyRouter<'a> {
    registry: &'a ProviderRegistry,
}

impl<'a> LowestLatencyRouter<'a> {
    /// Create a new lowest-latency router backed by the given registry.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ProviderRouter for LowestLatencyRouter<'_> {
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision> {
        let mut candidates = candidate_providers(self.registry, ctx);
        sort_by_numeric_hint(
            &mut candidates,
            ctx.preferred_model.as_deref(),
            "observed_latency_ms",
            false,
        );
        finalize_decision(candidates, ctx.fallback_allowed)
    }
}

/// A router that distributes requests across matching providers in round-robin order.
pub struct RoundRobinRouter<'a> {
    registry: &'a ProviderRegistry,
    cursor: AtomicUsize,
}

impl<'a> RoundRobinRouter<'a> {
    /// Create a new round-robin router backed by the given registry.
    pub fn new(registry: &'a ProviderRegistry) -> Self {
        Self {
            registry,
            cursor: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ProviderRouter for RoundRobinRouter<'_> {
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision> {
        let mut candidates = candidate_providers(self.registry, ctx);
        if candidates.is_empty() {
            return finalize_decision(candidates, ctx.fallback_allowed);
        }

        let index = self.cursor.fetch_add(1, AtomicOrdering::Relaxed) % candidates.len();
        candidates.rotate_left(index);
        finalize_decision(candidates, ctx.fallback_allowed)
    }
}

/// Router that delegates to one of the built-in strategies.
pub struct StrategyRouter<'a> {
    strategy: RoutingStrategy,
    registry: &'a ProviderRegistry,
    round_robin_cursor: AtomicUsize,
}

impl<'a> StrategyRouter<'a> {
    /// Create a new configurable built-in router.
    pub fn new(strategy: RoutingStrategy, registry: &'a ProviderRegistry) -> Self {
        Self {
            strategy,
            registry,
            round_robin_cursor: AtomicUsize::new(0),
        }
    }

    /// Return the configured strategy.
    pub fn strategy(&self) -> RoutingStrategy {
        self.strategy
    }
}

#[async_trait]
impl ProviderRouter for StrategyRouter<'_> {
    async fn route(&self, ctx: &RoutingContext) -> SwarmResult<RoutingDecision> {
        match self.strategy {
            RoutingStrategy::CapabilityMatch => {
                CapabilityMatchRouter::new(self.registry).route(ctx).await
            }
            RoutingStrategy::LowestCost => LowestCostRouter::new(self.registry).route(ctx).await,
            RoutingStrategy::LowestLatency => {
                LowestLatencyRouter::new(self.registry).route(ctx).await
            }
            RoutingStrategy::RoundRobin => {
                let mut candidates = candidate_providers(self.registry, ctx);
                if candidates.is_empty() {
                    return finalize_decision(candidates, ctx.fallback_allowed);
                }
                let index = self
                    .round_robin_cursor
                    .fetch_add(1, AtomicOrdering::Relaxed)
                    % candidates.len();
                candidates.rotate_left(index);
                finalize_decision(candidates, ctx.fallback_allowed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        request::{ChatRequest, EmbeddingRequest},
        response::{ChatResponse, EmbeddingResponse},
        streaming::StreamEvent,
        traits::ProviderHealth,
    };

    struct TestProvider {
        id: PluginId,
        name: &'static str,
        capabilities: ProviderCapabilities,
    }

    impl TestProvider {
        fn new(name: &'static str, cost: f64, latency_ms: f64, model_id: &str) -> Self {
            Self {
                id: PluginId::new(),
                name,
                capabilities: ProviderCapabilities {
                    chat_completion: true,
                    models: vec![crate::capabilities::ModelDescriptor {
                        model_id: model_id.into(),
                        display_name: model_id.into(),
                        max_context_tokens: Some(8192),
                        max_output_tokens: Some(2048),
                        supports_tools: true,
                        supports_vision: false,
                        supports_streaming: true,
                        supports_json_mode: true,
                        is_reasoning_model: false,
                    }],
                    custom: [
                        (
                            "estimated_cost_per_1k_tokens".into(),
                            serde_json::json!(cost),
                        ),
                        ("observed_latency_ms".into(), serde_json::json!(latency_ms)),
                    ]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                },
            }
        }
    }

    #[async_trait]
    impl ModelProvider for TestProvider {
        fn id(&self) -> PluginId {
            self.id
        }

        fn name(&self) -> &str {
            self.name
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.capabilities.clone()
        }

        async fn chat_completion(&self, request: ChatRequest) -> SwarmResult<ChatResponse> {
            Ok(ChatResponse {
                model: request.model,
                content: Some("ok".into()),
                tool_calls: Vec::new(),
                finish_reason: None,
                usage: None,
                response_id: None,
                extra: serde_json::Value::Null,
            })
        }

        async fn chat_completion_stream(
            &self,
            _request: ChatRequest,
        ) -> SwarmResult<Vec<StreamEvent>> {
            Ok(Vec::new())
        }

        async fn embedding(&self, request: EmbeddingRequest) -> SwarmResult<EmbeddingResponse> {
            Ok(EmbeddingResponse {
                model: request.model,
                embeddings: Vec::new(),
                usage: None,
            })
        }

        async fn health_check(&self) -> SwarmResult<ProviderHealth> {
            Ok(ProviderHealth {
                healthy: true,
                latency_ms: None,
                message: None,
            })
        }
    }

    fn seeded_registry() -> ProviderRegistry {
        let registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::new(
                "cheap",
                0.5,
                250.0,
                "gpt-cheap",
            )))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new("fast", 1.5, 80.0, "gpt-fast")))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new(
                "balanced",
                1.0,
                120.0,
                "gpt-balanced",
            )))
            .unwrap();
        registry
    }

    #[test]
    fn routing_context_defaults() {
        let ctx = RoutingContext::default();
        assert!(ctx.fallback_allowed);
        assert!(ctx.allowlist.is_empty());
        assert!(ctx.blocklist.is_empty());
    }

    #[tokio::test]
    async fn lowest_cost_router_prefers_cheapest_provider() {
        let registry = seeded_registry();
        let router = LowestCostRouter::new(&registry);

        let decision = router.route(&RoutingContext::default()).await.unwrap();

        assert_eq!(decision.provider.name(), "cheap");
    }

    #[tokio::test]
    async fn lowest_latency_router_prefers_fastest_provider() {
        let registry = seeded_registry();
        let router = LowestLatencyRouter::new(&registry);

        let decision = router.route(&RoutingContext::default()).await.unwrap();

        assert_eq!(decision.provider.name(), "fast");
    }

    #[tokio::test]
    async fn round_robin_router_rotates_providers() {
        let registry = seeded_registry();
        let router = RoundRobinRouter::new(&registry);

        let first = router.route(&RoutingContext::default()).await.unwrap();
        let second = router.route(&RoutingContext::default()).await.unwrap();

        assert_ne!(first.provider.id(), second.provider.id());
    }

    #[tokio::test]
    async fn strategy_router_delegates_to_selected_strategy() {
        let registry = seeded_registry();
        let router = StrategyRouter::new(RoutingStrategy::LowestLatency, &registry);

        let decision = router.route(&RoutingContext::default()).await.unwrap();

        assert_eq!(decision.provider.name(), "fast");
        assert_eq!(router.strategy(), RoutingStrategy::LowestLatency);
    }

    #[tokio::test]
    async fn preferred_model_is_honored_when_available() {
        let registry = seeded_registry();
        let router = LowestCostRouter::new(&registry);
        let ctx = RoutingContext {
            preferred_model: Some("gpt-fast".into()),
            ..RoutingContext::default()
        };

        let decision = router.route(&ctx).await.unwrap();

        assert_eq!(decision.provider.name(), "fast");
    }
}
