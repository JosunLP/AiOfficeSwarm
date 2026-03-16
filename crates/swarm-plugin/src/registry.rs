//! Plugin registry: tracks all loaded plugins and their state.

use dashmap::DashMap;
use std::sync::Arc;

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::PluginId,
};

use crate::{
    lifecycle::PluginState,
    manifest::PluginManifest,
};

/// A record combining a plugin's manifest with its live state.
#[derive(Debug, Clone)]
pub struct PluginRecord {
    /// The plugin's static manifest.
    pub manifest: PluginManifest,
    /// Current lifecycle state.
    pub state: PluginState,
}

/// Thread-safe registry of all plugins known to the host.
#[derive(Clone, Default)]
pub struct PluginRegistry {
    plugins: Arc<DashMap<PluginId, PluginRecord>>,
}

impl PluginRegistry {
    /// Create an empty plugin registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a plugin (transitions to `Discovered` state).
    pub fn register(&self, manifest: PluginManifest) -> SwarmResult<PluginId> {
        let id = manifest.id;
        if self.plugins.contains_key(&id) {
            return Err(SwarmError::Internal {
                reason: format!("plugin {} is already registered", id),
            });
        }
        let record = PluginRecord {
            manifest,
            state: PluginState::Discovered,
        };
        self.plugins.insert(id, record);
        Ok(id)
    }

    /// Update the state of a registered plugin.
    pub fn update_state(&self, id: &PluginId, state: PluginState) -> SwarmResult<()> {
        let mut record = self.plugins.get_mut(id).ok_or_else(|| SwarmError::Internal {
            reason: format!("plugin {} not found in registry", id),
        })?;
        record.state = state;
        Ok(())
    }

    /// Retrieve a plugin record by ID.
    pub fn get(&self, id: &PluginId) -> Option<PluginRecord> {
        self.plugins.get(id).map(|r| r.clone())
    }

    /// Return all plugin records.
    pub fn all(&self) -> Vec<PluginRecord> {
        self.plugins.iter().map(|r| r.clone()).collect()
    }

    /// Return all active plugins.
    pub fn active_plugins(&self) -> Vec<PluginRecord> {
        self.plugins
            .iter()
            .filter(|r| r.state.is_active())
            .map(|r| r.clone())
            .collect()
    }

    /// Deregister a plugin.
    pub fn deregister(&self, id: &PluginId) -> SwarmResult<()> {
        self.plugins.remove(id).map(|_| ()).ok_or_else(|| SwarmError::Internal {
            reason: format!("plugin {} not found", id),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn make_manifest(name: &str) -> PluginManifest {
        PluginManifest::new(name, "1.0.0", "test-author", "test plugin")
    }

    #[test]
    fn register_and_retrieve() {
        let reg = PluginRegistry::new();
        let manifest = make_manifest("my-plugin");
        let id = reg.register(manifest.clone()).unwrap();
        let record = reg.get(&id).unwrap();
        assert_eq!(record.manifest.name, "my-plugin");
        assert_eq!(record.state.label(), "discovered");
    }

    #[test]
    fn update_state_to_active() {
        let reg = PluginRegistry::new();
        let id = reg.register(make_manifest("plugin")).unwrap();
        reg.update_state(&id, PluginState::Active).unwrap();
        assert!(reg.get(&id).unwrap().state.is_active());
        assert_eq!(reg.active_plugins().len(), 1);
    }
}
