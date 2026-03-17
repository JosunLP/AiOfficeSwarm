//! Plugin manifest: static metadata describing a plugin's identity and
//! declared capabilities.
//!
//! The manifest is the first thing the host reads when loading a plugin.
//! It is used for version compatibility checks, capability discovery,
//! and permission metadata inspection.
//!
//! ## Manifest types
//! - [`PluginManifest`] – in-memory manifest used by the host at runtime.
//! - [`WasmPermission`] – OS-level sandbox permission declared by a WASM plugin.
//!
//! WASM plugins also supply a _file-based_ manifest (TOML) that is parsed by
//! [`crate::wasm_manifest::WasmManifestFile`] and converted into a
//! [`PluginManifest`] before being handed to the [`crate::PluginHost`].

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

/// An OS-level sandbox permission that a WASM plugin is allowed to use.
///
/// These declarations describe the OS-level resources a WASM plugin expects to
/// access. The current host carries them through the manifest for inspection,
/// audit, or embedding-application enforcement; they are not yet enforced by
/// [`crate::wasm_loader::WasmPluginLoader`] during instantiation.
///
/// These are _separate_ from the framework-level RBAC permissions stored in
/// [`PluginManifest::required_permissions`], which control what actions the
/// plugin may perform inside the swarm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum WasmPermission {
    /// Outbound network access to a host/CIDR (e.g. `"api.example.com:443"`).
    Network(String),
    /// Read access to an environment variable (e.g. `"MY_API_KEY"`).
    EnvVar(String),
    /// Read access to a filesystem path (e.g. `"/etc/ssl/certs"`).
    FileRead(String),
    /// Write access to a filesystem path (e.g. `"/tmp/plugin-cache"`).
    FileWrite(String),
    /// An arbitrary named permission for custom sandbox enforcement.
    Custom(String),
}

impl WasmPermission {
    /// Returns a compact string representation suitable for logging or audit.
    ///
    /// ```text
    /// network:api.example.com:443
    /// env_var:MY_API_KEY
    /// file_read:/etc/ssl/certs
    /// file_write:/tmp/cache
    /// custom:my-permission
    /// ```
    pub fn as_str(&self) -> String {
        match self {
            WasmPermission::Network(v) => format!("network:{v}"),
            WasmPermission::EnvVar(v) => format!("env_var:{v}"),
            WasmPermission::FileRead(v) => format!("file_read:{v}"),
            WasmPermission::FileWrite(v) => format!("file_write:{v}"),
            WasmPermission::Custom(v) => format!("custom:{v}"),
        }
    }
}

/// The static manifest that every plugin must provide.
///
/// The host validates the manifest before calling [`Plugin::on_load`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Stable, unique plugin identifier backed by a UUID.
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
    /// Format: `"verb:resource"` (e.g., `"create:task"`). These declarations
    /// describe the framework permissions a plugin expects; enforcement is
    /// performed by the embedding application or a higher-level host wrapper.
    pub required_permissions: Vec<String>,
    /// OS-level sandbox permissions required by this WASM plugin.
    ///
    /// Empty for native (non-WASM) plugins. For WASM plugins these declare
    /// what system resources the sandboxed module expects to access.
    #[serde(default)]
    pub wasm_permissions: Vec<WasmPermission>,
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
            wasm_permissions: Vec::new(),
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

    #[test]
    fn wasm_permission_as_str() {
        assert_eq!(WasmPermission::Network("api.test.com:443".into()).as_str(), "network:api.test.com:443");
        assert_eq!(WasmPermission::EnvVar("MY_KEY".into()).as_str(), "env_var:MY_KEY");
        assert_eq!(WasmPermission::FileRead("/etc/ssl".into()).as_str(), "file_read:/etc/ssl");
        assert_eq!(WasmPermission::FileWrite("/tmp".into()).as_str(), "file_write:/tmp");
        assert_eq!(WasmPermission::Custom("special".into()).as_str(), "custom:special");
    }

    #[test]
    fn wasm_permission_roundtrip_json() {
        let perms = vec![
            WasmPermission::Network("api.example.com:443".into()),
            WasmPermission::EnvVar("API_KEY".into()),
        ];
        let json = serde_json::to_string(&perms).unwrap();
        let decoded: Vec<WasmPermission> = serde_json::from_str(&json).unwrap();
        assert_eq!(perms, decoded);
    }

    #[test]
    fn manifest_wasm_permissions_default_empty() {
        let m = PluginManifest::new("test", "1.0.0", "author", "desc");
        assert!(m.wasm_permissions.is_empty());
    }
}
