//! Plugin manifest: static metadata describing a plugin's identity and
//! declared capabilities.
//!
//! The manifest is the first thing the host reads when loading a plugin.
//! It is used for capability discovery, permission metadata inspection,
//! and embedding-application-managed compatibility checks.
//!
//! ## Manifest types
//! - [`PluginManifest`] – in-memory manifest used by the host at runtime.
//! - [`WasmPermission`] – OS-level sandbox permission declared by a WASM plugin.
//!
//! WASM plugins also supply a _file-based_ manifest (TOML) that is parsed by
//! [`crate::wasm_manifest::WasmManifestFile`] and converted into a
//! [`PluginManifest`] before being handed to the [`crate::PluginHost`].

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use swarm_core::error::{SwarmError, SwarmResult};
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

    // ── v2 capability kinds ────────────────────────────────────────────
    /// The plugin adapts an AI model provider (e.g., OpenAI, Anthropic).
    ProviderAdapter,
    /// The plugin provides a memory backend (e.g., vector DB, SQL store).
    MemoryBackend,
    /// The plugin contributes a learning strategy.
    LearningStrategy,
    /// The plugin bundles personality profiles and overlays.
    PersonalityPack,
    /// The plugin provides workflow templates.
    WorkflowProvider,
    /// The plugin connects an enterprise system (ERP, CRM, etc.).
    EnterpriseConnector,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub fn compact_string(&self) -> String {
        match self {
            WasmPermission::Network(v) => format!("network:{v}"),
            WasmPermission::EnvVar(v) => format!("env_var:{v}"),
            WasmPermission::FileRead(v) => format!("file_read:{v}"),
            WasmPermission::FileWrite(v) => format!("file_write:{v}"),
            WasmPermission::Custom(v) => format!("custom:{v}"),
        }
    }
}

impl std::fmt::Display for WasmPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.compact_string())
    }
}

/// The static manifest that every plugin must provide.
///
/// The host reads and registers this metadata before calling [`crate::Plugin::on_load`].
/// Compatibility checks and host-configured permission guardrails can be
/// validated by [`crate::PluginHost`], while richer embedding-application
/// RBAC/policy decisions can still wrap host calls.
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
    /// describe the framework permissions a plugin expects; host load and
    /// invocation policies may enforce them, and embedding applications may add
    /// higher-level contextual policy on top.
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

    /// Validate the manifest for basic structural correctness and host compatibility.
    pub fn validate_for_host(&self, host_version: &str) -> SwarmResult<()> {
        if self.name.trim().is_empty() {
            return Err(SwarmError::ConfigInvalid {
                key: "plugin.name".into(),
                reason: "plugin name must not be empty".into(),
            });
        }

        if self.author.trim().is_empty() {
            return Err(SwarmError::ConfigInvalid {
                key: format!("plugin.{}.author", self.name),
                reason: "plugin author must not be empty".into(),
            });
        }

        validate_semver_field(&self.version, &format!("plugin.{}.version", self.name))?;
        validate_semver_field(
            &self.min_host_version,
            &format!("plugin.{}.min_host_version", self.name),
        )?;

        if compare_semver_triplets(host_version, &self.min_host_version)?
            == std::cmp::Ordering::Less
        {
            return Err(SwarmError::PluginVersionMismatch {
                name: self.name.clone(),
                plugin_version: self.min_host_version.clone(),
                host_version: host_version.into(),
            });
        }

        let mut seen_actions = HashSet::new();
        for action in &self.actions {
            if action.name.trim().is_empty() {
                return Err(SwarmError::ConfigInvalid {
                    key: format!("plugin.{}.actions", self.name),
                    reason: "plugin action names must not be empty".into(),
                });
            }
            if !seen_actions.insert(action.name.to_ascii_lowercase()) {
                return Err(SwarmError::ConfigInvalid {
                    key: format!("plugin.{}.actions", self.name),
                    reason: format!("duplicate action '{}' declared", action.name),
                });
            }
        }

        if !self.actions.is_empty()
            && !self
                .capabilities
                .iter()
                .any(|capability| capability == &PluginCapabilityKind::ActionProvider)
        {
            return Err(SwarmError::ConfigInvalid {
                key: format!("plugin.{}.capabilities", self.name),
                reason: "plugins declaring actions must include the ActionProvider capability"
                    .into(),
            });
        }

        if self.actions.is_empty()
            && self
                .capabilities
                .iter()
                .any(|capability| capability == &PluginCapabilityKind::ActionProvider)
        {
            return Err(SwarmError::ConfigInvalid {
                key: format!("plugin.{}.actions", self.name),
                reason: "ActionProvider plugins must declare at least one action".into(),
            });
        }

        let mut seen_permissions = HashSet::new();
        for permission in &self.required_permissions {
            if !is_valid_permission(permission) {
                return Err(SwarmError::ConfigInvalid {
                    key: format!("plugin.{}.required_permissions", self.name),
                    reason: format!(
                        "permission '{}' must use the 'verb:resource' format",
                        permission
                    ),
                });
            }

            if !seen_permissions.insert(permission.to_ascii_lowercase()) {
                return Err(SwarmError::ConfigInvalid {
                    key: format!("plugin.{}.required_permissions", self.name),
                    reason: format!("duplicate required permission '{}' declared", permission),
                });
            }
        }

        Ok(())
    }
}

fn validate_semver_field(value: &str, key: &str) -> SwarmResult<()> {
    if value.trim().is_empty() {
        return Err(SwarmError::ConfigInvalid {
            key: key.into(),
            reason: "must not be empty".into(),
        });
    }

    parse_semver_triplet(value).map(|_| ())
}

fn compare_semver_triplets(left: &str, right: &str) -> SwarmResult<std::cmp::Ordering> {
    let left = parse_semver_triplet(left)?;
    let right = parse_semver_triplet(right)?;
    Ok(left.cmp(&right))
}

fn parse_semver_triplet(value: &str) -> SwarmResult<(u64, u64, u64)> {
    let cleaned = value.trim().trim_start_matches('v');
    let mut parts = cleaned.split('.');
    let parsed = (
        parts
            .next()
            .ok_or_else(|| invalid_semver(value))?
            .parse::<u64>()
            .map_err(|_| invalid_semver(value))?,
        parts
            .next()
            .ok_or_else(|| invalid_semver(value))?
            .parse::<u64>()
            .map_err(|_| invalid_semver(value))?,
        parts
            .next()
            .ok_or_else(|| invalid_semver(value))?
            .parse::<u64>()
            .map_err(|_| invalid_semver(value))?,
    );

    if parts.next().is_some() {
        return Err(invalid_semver(value));
    }

    Ok(parsed)
}

fn invalid_semver(value: &str) -> SwarmError {
    SwarmError::ConfigInvalid {
        key: "plugin.semver".into(),
        reason: format!("'{}' is not a valid semantic version triplet", value),
    }
}

fn is_valid_permission(value: &str) -> bool {
    let mut parts = value.split(':');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(verb), Some(resource), None) if !verb.trim().is_empty() && !resource.trim().is_empty()
    )
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
        assert_eq!(
            WasmPermission::Network("api.test.com:443".into()).compact_string(),
            "network:api.test.com:443"
        );
        assert_eq!(
            WasmPermission::EnvVar("MY_KEY".into()).compact_string(),
            "env_var:MY_KEY"
        );
        assert_eq!(
            WasmPermission::FileRead("/etc/ssl".into()).compact_string(),
            "file_read:/etc/ssl"
        );
        assert_eq!(
            WasmPermission::FileWrite("/tmp".into()).compact_string(),
            "file_write:/tmp"
        );
        assert_eq!(
            WasmPermission::Custom("special".into()).compact_string(),
            "custom:special"
        );
        assert_eq!(
            WasmPermission::Custom("special".into()).to_string(),
            "custom:special"
        );
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

    #[test]
    fn validate_manifest_rejects_duplicate_actions() {
        let mut manifest = PluginManifest::new("test", "1.0.0", "author", "desc");
        manifest
            .capabilities
            .push(PluginCapabilityKind::ActionProvider);
        manifest.actions.push(PluginAction {
            name: "echo".into(),
            description: "first".into(),
            input_schema: None,
            output_schema: None,
        });
        manifest.actions.push(PluginAction {
            name: "echo".into(),
            description: "second".into(),
            input_schema: None,
            output_schema: None,
        });

        assert!(matches!(
            manifest.validate_for_host("0.1.0"),
            Err(SwarmError::ConfigInvalid { reason, .. }) if reason.contains("duplicate action")
        ));
    }

    #[test]
    fn validate_manifest_rejects_invalid_permission_format() {
        let mut manifest = PluginManifest::new("test", "1.0.0", "author", "desc");
        manifest.required_permissions.push("read-config".into());

        assert!(matches!(
            manifest.validate_for_host("0.1.0"),
            Err(SwarmError::ConfigInvalid { reason, .. }) if reason.contains("verb:resource")
        ));
    }

    #[test]
    fn validate_manifest_rejects_incompatible_host_version() {
        let manifest = PluginManifest {
            min_host_version: "9.9.9".into(),
            ..PluginManifest::new("test", "1.0.0", "author", "desc")
        };

        assert!(matches!(
            manifest.validate_for_host("0.1.0"),
            Err(SwarmError::PluginVersionMismatch { .. })
        ));
    }
}
