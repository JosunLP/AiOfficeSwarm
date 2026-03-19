//! Runtime registry of available AI model providers.
//!
//! The [`ProviderRegistry`] stores registered [`ModelProvider`] instances and
//! allows lookup by ID or by capability requirements.

use std::sync::Arc;

use dashmap::DashMap;
use tracing;

use swarm_core::error::{SwarmError, SwarmResult};
use swarm_core::identity::PluginId;

use crate::capabilities::ProviderCapabilities;
use crate::traits::ModelProvider;

/// Container for a registered provider and its snapshot of capabilities.
#[derive(Clone)]
struct ProviderEntry {
    provider: Arc<dyn ModelProvider>,
    capabilities: ProviderCapabilities,
}

/// Thread-safe registry of AI model providers.
///
/// Providers are stored behind `Arc` so they can be shared across async tasks.
pub struct ProviderRegistry {
    entries: DashMap<PluginId, ProviderEntry>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Register a provider. Returns an error if a provider with the same ID
    /// is already registered.
    pub fn register(&self, provider: Arc<dyn ModelProvider>) -> SwarmResult<()> {
        let id = provider.id();
        let capabilities = provider.capabilities();
        let name = provider.name().to_string();

        if self.entries.contains_key(&id) {
            return Err(SwarmError::Internal {
                reason: format!("Provider '{}' ({}) is already registered", name, id),
            });
        }

        self.entries.insert(
            id,
            ProviderEntry {
                provider,
                capabilities,
            },
        );
        tracing::info!(provider_id = %id, name = %name, "Provider registered");
        Ok(())
    }

    /// Deregister a provider by ID.
    pub fn deregister(&self, id: &PluginId) -> SwarmResult<()> {
        self.entries
            .remove(id)
            .ok_or_else(|| SwarmError::Internal {
                reason: format!("Provider {} not found in registry", id),
            })?;
        tracing::info!(provider_id = %id, "Provider deregistered");
        Ok(())
    }

    /// Look up a provider by ID.
    pub fn get(&self, id: &PluginId) -> Option<Arc<dyn ModelProvider>> {
        self.entries.get(id).map(|e| Arc::clone(&e.provider))
    }

    /// Look up a provider by its stable short name.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn ModelProvider>> {
        self.entries
            .iter()
            .find(|entry| entry.value().provider.name().eq_ignore_ascii_case(name))
            .map(|entry| Arc::clone(&entry.value().provider))
    }

    /// Resolve a provider ID by its stable short name.
    pub fn id_by_name(&self, name: &str) -> Option<PluginId> {
        self.entries
            .iter()
            .find(|entry| entry.value().provider.name().eq_ignore_ascii_case(name))
            .map(|entry| *entry.key())
    }

    /// Find all providers that satisfy the given capability requirements.
    pub fn find_by_capabilities(
        &self,
        required: &ProviderCapabilities,
    ) -> Vec<Arc<dyn ModelProvider>> {
        self.entries
            .iter()
            .filter(|e| e.value().capabilities.satisfies(required))
            .map(|e| Arc::clone(&e.value().provider))
            .collect()
    }

    /// Return the number of registered providers.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the IDs and names of all registered providers.
    pub fn list(&self) -> Vec<(PluginId, String)> {
        self.entries
            .iter()
            .map(|e| (*e.key(), e.value().provider.name().to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry() {
        let reg = ProviderRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn lookup_unknown_provider_name_returns_none() {
        let reg = ProviderRegistry::new();
        assert!(reg.get_by_name("missing").is_none());
        assert!(reg.id_by_name("missing").is_none());
    }
}
