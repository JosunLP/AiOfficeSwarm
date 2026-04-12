//! Provider selection and routing strategies.
//!
//! The [`ProviderRouter`] trait defines how the framework selects a provider
//! for a given request. Implementations can consider capabilities, cost,
//! latency, compliance, and tenant preferences.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
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
    /// Whether unhealthy providers should be excluded from routing decisions.
    pub require_healthy: bool,
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
            require_healthy: false,
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

fn compare_numeric_hint(
    a: &Arc<dyn ModelProvider>,
    b: &Arc<dyn ModelProvider>,
    key: &str,
    descending: bool,
) -> Ordering {
    match (numeric_hint(a, key), numeric_hint(b, key)) {
        (Some(a_score), Some(b_score)) => if descending {
            b_score.partial_cmp(&a_score)
        } else {
            a_score.partial_cmp(&b_score)
        }
        .unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn sort_capability_match_candidates(
    candidates: &mut [Arc<dyn ModelProvider>],
    ctx: &RoutingContext,
) {
    candidates.sort_by(|a, b| {
        let a_model = provider_matches_model(a, ctx.preferred_model.as_deref());
        let b_model = provider_matches_model(b, ctx.preferred_model.as_deref());
        b_model
            .cmp(&a_model)
            .then_with(|| match ctx.cost_preference {
                CostPreference::Cheapest => {
                    compare_numeric_hint(a, b, "estimated_cost_per_1k_tokens", false)
                }
                CostPreference::BestQuality => compare_numeric_hint(a, b, "quality_tier", true),
                CostPreference::Balanced => Ordering::Equal,
            })
            .then_with(|| match ctx.latency_preference {
                LatencyPreference::Fastest => {
                    compare_numeric_hint(a, b, "observed_latency_ms", false)
                }
                LatencyPreference::Balanced | LatencyPreference::NoPreference => Ordering::Equal,
            })
            .then_with(|| a.name().cmp(b.name()))
    });
}

fn normalized_string_set(value: &serde_json::Value) -> HashSet<String> {
    match value {
        serde_json::Value::String(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect(),
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect(),
        _ => HashSet::new(),
    }
}

fn provider_custom_string_set(provider: &Arc<dyn ModelProvider>, keys: &[&str]) -> HashSet<String> {
    let capabilities = provider.capabilities();
    keys.iter()
        .find_map(|key| capabilities.custom.get(*key).map(normalized_string_set))
        .unwrap_or_default()
}

fn provider_meets_compliance(
    provider: &Arc<dyn ModelProvider>,
    requirements: &[ComplianceRequirement],
) -> bool {
    if requirements.is_empty() {
        return true;
    }

    let supported = provider_custom_string_set(provider, &["compliance_standards", "compliance"]);
    if supported.is_empty() {
        return false;
    }

    requirements
        .iter()
        .all(|requirement| supported.contains(&requirement.standard.trim().to_ascii_lowercase()))
}

fn provider_matches_data_locality(
    provider: &Arc<dyn ModelProvider>,
    data_locality: &DataLocality,
) -> bool {
    if data_locality.allowed_regions.is_empty() {
        return true;
    }

    let supported_regions = provider_custom_string_set(provider, &["regions", "region"]);
    if supported_regions.is_empty() {
        return !data_locality.strict;
    }

    data_locality
        .allowed_regions
        .iter()
        .map(|region| region.trim().to_ascii_lowercase())
        .any(|region| supported_regions.contains(&region))
}

async fn finalize_decision(
    mut candidates: Vec<Arc<dyn ModelProvider>>,
    ctx: &RoutingContext,
) -> SwarmResult<RoutingDecision> {
    if ctx.require_healthy {
        let mut healthy_candidates = Vec::with_capacity(candidates.len());
        for provider in candidates {
            match provider.health_check().await {
                Ok(health) if health.healthy => healthy_candidates.push(provider),
                Ok(_) | Err(_) => {}
            }
        }
        candidates = healthy_candidates;
    }

    if candidates.is_empty() {
        return Err(SwarmError::ProviderRoutingFailed {
            reason: if ctx.require_healthy {
                "no healthy provider matches the routing requirements".into()
            } else {
                "no provider matches the routing requirements".into()
            },
        });
    }

    let primary = candidates.remove(0);
    Ok(RoutingDecision {
        provider: primary,
        fallbacks: if ctx.fallback_allowed {
            candidates
        } else {
            Vec::new()
        },
    })
}

fn candidate_providers(
    registry: &ProviderRegistry,
    ctx: &RoutingContext,
) -> SwarmResult<Vec<Arc<dyn ModelProvider>>> {
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
            candidates = preferred;
        }
    }

    if let Some(data_locality) = &ctx.data_locality {
        let localized: Vec<_> = candidates
            .iter()
            .filter(|provider| provider_matches_data_locality(provider, data_locality))
            .cloned()
            .collect();

        if data_locality.strict {
            if localized.is_empty() {
                return Err(SwarmError::ComplianceBoundaryViolation {
                    reason: format!(
                        "no provider satisfies the required data locality: {}",
                        data_locality.allowed_regions.join(", ")
                    ),
                });
            }
            candidates = localized;
        } else if !localized.is_empty() {
            candidates = localized;
        }
    }

    let mandatory_compliance: Vec<_> = ctx
        .compliance
        .iter()
        .filter(|requirement| requirement.mandatory)
        .cloned()
        .collect();
    if !mandatory_compliance.is_empty() {
        candidates.retain(|provider| provider_meets_compliance(provider, &mandatory_compliance));
        if candidates.is_empty() {
            return Err(SwarmError::ComplianceBoundaryViolation {
                reason: format!(
                    "no provider satisfies mandatory compliance requirements: {}",
                    mandatory_compliance
                        .iter()
                        .map(|requirement| requirement.standard.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }

    let advisory_compliance: Vec<_> = ctx
        .compliance
        .iter()
        .filter(|requirement| !requirement.mandatory)
        .cloned()
        .collect();
    if !advisory_compliance.is_empty() {
        let preferred: Vec<_> = candidates
            .iter()
            .filter(|provider| provider_meets_compliance(provider, &advisory_compliance))
            .cloned()
            .collect();
        if !preferred.is_empty() {
            candidates = preferred;
        }
    }

    Ok(candidates)
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
        let mut candidates = candidate_providers(self.registry, ctx)?;
        sort_capability_match_candidates(&mut candidates, ctx);
        finalize_decision(candidates, ctx).await
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
        let mut candidates = candidate_providers(self.registry, ctx)?;
        sort_by_numeric_hint(
            &mut candidates,
            ctx.preferred_model.as_deref(),
            "estimated_cost_per_1k_tokens",
            false,
        );
        finalize_decision(candidates, ctx).await
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
        let mut candidates = candidate_providers(self.registry, ctx)?;
        sort_by_numeric_hint(
            &mut candidates,
            ctx.preferred_model.as_deref(),
            "observed_latency_ms",
            false,
        );
        finalize_decision(candidates, ctx).await
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
        let mut candidates = candidate_providers(self.registry, ctx)?;
        if candidates.is_empty() {
            return finalize_decision(candidates, ctx).await;
        }

        let index = self.cursor.fetch_add(1, AtomicOrdering::Relaxed) % candidates.len();
        candidates.rotate_left(index);
        finalize_decision(candidates, ctx).await
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
                let mut candidates = candidate_providers(self.registry, ctx)?;
                if candidates.is_empty() {
                    return finalize_decision(candidates, ctx).await;
                }
                let index = self
                    .round_robin_cursor
                    .fetch_add(1, AtomicOrdering::Relaxed)
                    % candidates.len();
                candidates.rotate_left(index);
                finalize_decision(candidates, ctx).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
        healthy: bool,
    }

    impl TestProvider {
        fn new(name: &'static str, cost: f64, latency_ms: f64, model_id: &str) -> Self {
            Self::with_metadata(name, cost, latency_ms, model_id, true, HashMap::new())
        }

        fn with_metadata(
            name: &'static str,
            cost: f64,
            latency_ms: f64,
            model_id: &str,
            healthy: bool,
            mut custom: HashMap<String, serde_json::Value>,
        ) -> Self {
            custom
                .entry("estimated_cost_per_1k_tokens".into())
                .or_insert_with(|| serde_json::json!(cost));
            custom
                .entry("observed_latency_ms".into())
                .or_insert_with(|| serde_json::json!(latency_ms));

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
                    custom,
                    ..Default::default()
                },
                healthy,
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
                healthy: self.healthy,
                latency_ms: self
                    .capabilities
                    .custom
                    .get("observed_latency_ms")
                    .and_then(serde_json::Value::as_f64)
                    .map(|latency| latency as u64),
                message: Some(format!("{} health", self.name)),
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
        assert!(!ctx.require_healthy);
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

    #[tokio::test]
    async fn capability_match_router_uses_cost_and_latency_preferences() {
        let registry = seeded_registry();
        let router = CapabilityMatchRouter::new(&registry);
        let ctx = RoutingContext {
            cost_preference: CostPreference::Cheapest,
            latency_preference: LatencyPreference::Fastest,
            ..RoutingContext::default()
        };

        let decision = router.route(&ctx).await.unwrap();

        assert_eq!(decision.provider.name(), "cheap");
    }

    #[tokio::test]
    async fn advisory_compliance_prefers_matching_provider() {
        let registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::with_metadata(
                "compliant",
                1.0,
                100.0,
                "gpt-compliant",
                true,
                HashMap::from([(
                    "compliance_standards".into(),
                    serde_json::json!(["SOC2", "GDPR"]),
                )]),
            )))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new(
                "generic",
                0.8,
                90.0,
                "gpt-generic",
            )))
            .unwrap();

        let decision = CapabilityMatchRouter::new(&registry)
            .route(&RoutingContext {
                compliance: vec![ComplianceRequirement {
                    standard: "soc2".into(),
                    mandatory: false,
                }],
                ..RoutingContext::default()
            })
            .await
            .unwrap();

        assert_eq!(decision.provider.name(), "compliant");
    }

    #[tokio::test]
    async fn strict_data_locality_rejects_out_of_region_providers() {
        let registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::with_metadata(
                "us-only",
                1.0,
                100.0,
                "gpt-us",
                true,
                HashMap::from([("regions".into(), serde_json::json!(["us-east-1"]))]),
            )))
            .unwrap();

        let err = CapabilityMatchRouter::new(&registry)
            .route(&RoutingContext {
                data_locality: Some(DataLocality {
                    allowed_regions: vec!["eu-west-1".into()],
                    strict: true,
                }),
                ..RoutingContext::default()
            })
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            SwarmError::ComplianceBoundaryViolation { .. }
        ));
    }

    #[tokio::test]
    async fn require_healthy_filters_unhealthy_providers() {
        let registry = ProviderRegistry::new();
        registry
            .register(Arc::new(TestProvider::with_metadata(
                "unhealthy-cheap",
                0.1,
                100.0,
                "gpt-cheap",
                false,
                HashMap::new(),
            )))
            .unwrap();
        registry
            .register(Arc::new(TestProvider::new(
                "healthy-fallback",
                1.0,
                120.0,
                "gpt-healthy",
            )))
            .unwrap();

        let decision = LowestCostRouter::new(&registry)
            .route(&RoutingContext {
                require_healthy: true,
                ..RoutingContext::default()
            })
            .await
            .unwrap();

        assert_eq!(decision.provider.name(), "healthy-fallback");
    }
}
