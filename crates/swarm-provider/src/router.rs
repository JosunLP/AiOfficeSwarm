//! Provider selection and routing strategies.
//!
//! The [`ProviderRouter`] trait defines how the framework selects a provider
//! for a given request. Implementations can consider capabilities, cost,
//! latency, compliance, and tenant preferences.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use swarm_core::error::SwarmResult;
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
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
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
        let mut candidates = self
            .registry
            .find_by_capabilities(&ctx.required_capabilities);

        // Apply allowlist.
        if !ctx.allowlist.is_empty() {
            candidates.retain(|p| ctx.allowlist.contains(&p.id()));
        }
        // Apply blocklist.
        if !ctx.blocklist.is_empty() {
            candidates.retain(|p| !ctx.blocklist.contains(&p.id()));
        }

        if candidates.is_empty() {
            return Err(swarm_core::error::SwarmError::Internal {
                reason: "No provider matches the routing requirements".into(),
            });
        }

        let primary = candidates.remove(0);
        Ok(RoutingDecision {
            provider: primary,
            fallbacks: candidates,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_context_defaults() {
        let ctx = RoutingContext::default();
        assert!(ctx.fallback_allowed);
        assert!(ctx.allowlist.is_empty());
        assert!(ctx.blocklist.is_empty());
    }
}
