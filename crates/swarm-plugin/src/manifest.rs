//! Plugin manifest: static metadata describing a plugin's identity and
//! declared capabilities.
//!
//! The manifest is the first thing the host reads when loading a plugin.
//! It is used for version compatibility checks, permission validation,
//! and capability discovery.

use serde::{Deserialize, Serialize};
use swarm_core::identity::PluginId;

/// Describes the type of capability a plugin provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginCapabilityKind {
    /// The plugin registers one or more agent types.
    AgentProvider,
    /// The plugin adds callable actions.
    ActionProvider,
    /// The plugin provides a storage backend.
    StorageBackend,
    /// The plugin connects an external communication channel.
    CommunicationChannel,
    /// The plugin contributes policy rules.
    PolicyProvider,
    /// The plugin reacts to external events (webhooks, schedules, etc.).
    TriggerProvider,
}

/// A named action exposed by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAction {
    /// The action name used to invoke it.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema (as a free-form Value) describing the expected input.
    pub input_schema: Option<serde_json::Value>,
    /// JSON Schema describing the output.
    pub output_schema: Option<serde_json::Value>,
}

/// The static manifest that every plugin must provide.
///
/// The host validates the manifest before calling [`Plugin::on_load`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Stable, unique plugin identifier (e.g., `"com.example.github-integration"`).
    pub id: PluginId,
    /// Human-readable plugin name.
    pub name: String,
    /// Semantic version string of the plugin (e.g., `"1.0.0"`).
    pub version: String,
    /// Plugin author or vendor name.
    pub author: String,
    /// Short description of the plugin's purpose.
    pub description: String,
    /// Minimum host framework version required (semver).
    pub min_host_version: String,
    /// Capabilities this plugin provides.
    pub capabilities: Vec<PluginCapabilityKind>,
    /// Actions this plugin exposes.
    pub actions: Vec<PluginAction>,
    /// Framework permissions required by this plugin.
    ///
    /// Format: `"verb:resource"` (e.g., `"create:task"`). The host checks
    /// these against RBAC before loading the plugin.
    pub required_permissions: Vec<String>,
}

impl PluginManifest {
    /// Create a minimal manifest with sensible defaults.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        author: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: PluginId::new(),
            name: name.into(),
            version: version.into(),
            author: author.into(),
            description: description.into(),
            min_host_version: "0.1.0".into(),
            capabilities: Vec::new(),
            actions: Vec::new(),
            required_permissions: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_unique_id() {
        let a = PluginManifest::new("plugin-a", "1.0.0", "author", "desc");
        let b = PluginManifest::new("plugin-b", "1.0.0", "author", "desc");
        assert_ne!(a.id, b.id);
    }
}
