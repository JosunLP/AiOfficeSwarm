//! WASM plugin manifest: file-based metadata format for precompiled WASM plugins.
//!
//! A WASM plugin is distributed as a pair of files:
//! - `<name>.wasm` — the precompiled WebAssembly binary
//! - `plugin.toml` — the manifest file described by this module
//!
//! The manifest is written in TOML and declares the plugin's identity,
//! capabilities, actions, permissions, and the relative path to the `.wasm`
//! binary. The host parses it with [`WasmManifestFile::load`] and converts
//! it to a [`PluginManifest`] for the rest of the framework.
//!
//! ## Manifest format
//!
//! ```toml
//! # Example WASM plugin manifest
//!
//! [plugin]
//! name            = "My WASM Plugin"
//! version         = "1.0.0"
//! author          = "Acme Corp"
//! description     = "A useful plugin compiled to WebAssembly"
//! min_host_version = "0.1.0"
//! capabilities    = ["ActionProvider"]
//!
//! # Path to the .wasm binary, relative to this manifest file.
//! wasm_file = "my-plugin.wasm"
//!
//! # Optional stable plugin ID (UUID). Auto-generated when omitted.
//! # id = "550e8400-e29b-41d4-a716-446655440000"
//!
//! # Framework-level RBAC permissions (verb:resource pairs).
//! required_permissions = ["read:config"]
//!
//! # OS-level sandbox permissions for the WASM module.
//! [[plugin.wasm_permissions]]
//! kind  = "Network"
//! value = "api.example.com:443"
//!
//! [[plugin.wasm_permissions]]
//! kind  = "EnvVar"
//! value = "MY_API_KEY"
//!
//! # Actions this plugin exposes.
//! [[plugin.actions]]
//! name        = "do_something"
//! description = "Performs a useful operation"
//! ```
//!
//! ## WASM ABI
//!
//! The `.wasm` module must export the following functions so the host can
//! drive it through its lifecycle:
//!
//! | Export | Signature | Description |
//! |--------|-----------|-------------|
//! | `memory` | (memory) | The module's linear memory |
//! | `swarm_alloc` | `(size: i32) -> i32` | Allocate `size` bytes; returns pointer |
//! | `swarm_dealloc` | `(ptr: i32, len: i32)` | Free `len` bytes at `ptr` |
//! | `swarm_on_load` | `() -> i32` | Called once after load. `0` = success |
//! | `swarm_on_unload` | `() -> i32` | Called before unload. `0` = success |
//! | `swarm_health_check` | `() -> i32` | `0` = healthy |
//! | `swarm_invoke` | `(action_ptr: i32, action_len: i32, params_ptr: i32, params_len: i32, result_ptr: i32, result_cap: i32) -> i32` | Invoke an action. See below |
//!
//! ### `swarm_invoke` return convention
//!
//! - **`n >= 0`**: success — `n` bytes of valid UTF-8 JSON were written to
//!   `result_ptr`.
//! - **`n < 0`**: error — `(-n)` bytes of a UTF-8 error message were written
//!   to `result_ptr`. If `n == -1` no message bytes were written.
//!
//! ### Memory protocol
//!
//! The host uses `swarm_alloc` / `swarm_dealloc` to manage memory inside the
//! WASM module's linear memory when passing strings to it:
//!
//! 1. Host calls `swarm_alloc(len)` → gets a pointer `ptr`.
//! 2. Host writes `len` bytes of data into `memory[ptr..ptr+len]`.
//! 3. Host calls the WASM function with `ptr` + `len`.
//! 4. Host calls `swarm_dealloc(ptr, len)` when done.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::PluginId,
};

use crate::manifest::{PluginAction, PluginCapabilityKind, PluginManifest, WasmPermission};

// ─── File-format types ────────────────────────────────────────────────────────

/// Root structure of the on-disk TOML manifest.
///
/// Deserialise with [`WasmManifestFile::load`] or from a TOML string with
/// `toml::from_str::<WasmManifestFile>(...)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmManifestFile {
    /// All plugin metadata lives under the `[plugin]` table.
    pub plugin: WasmPluginSection,
}

/// The `[plugin]` table in the manifest TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPluginSection {
    /// Stable plugin UUID. Auto-generated when absent.
    pub id: Option<PluginId>,

    /// Human-readable plugin name.
    pub name: String,

    /// Semantic version (e.g. `"1.2.3"`).
    pub version: String,

    /// Author or vendor name.
    pub author: String,

    /// Short description of the plugin's purpose.
    pub description: String,

    /// Minimum host framework version required (default: `"0.1.0"`).
    #[serde(default = "default_min_host_version")]
    pub min_host_version: String,

    /// Path to the `.wasm` file, **relative to the manifest file's directory**.
    pub wasm_file: PathBuf,

    /// Capability kinds provided by this plugin.
    #[serde(default)]
    pub capabilities: Vec<PluginCapabilityKind>,

    /// Actions exposed by this plugin.
    #[serde(default)]
    pub actions: Vec<WasmActionEntry>,

    /// Framework-level RBAC permissions (`"verb:resource"` pairs).
    #[serde(default)]
    pub required_permissions: Vec<String>,

    /// OS-level sandbox permissions for the WASM module.
    #[serde(default)]
    pub wasm_permissions: Vec<WasmPermission>,
}

/// One entry in the `[[plugin.actions]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmActionEntry {
    /// Action name (must match the string passed to `swarm_invoke`).
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// Optional JSON Schema for input validation.
    pub input_schema: Option<serde_json::Value>,

    /// Optional JSON Schema for output documentation.
    pub output_schema: Option<serde_json::Value>,
}

fn default_min_host_version() -> String {
    "0.1.0".into()
}

// ─── Loading ──────────────────────────────────────────────────────────────────

impl WasmManifestFile {
    /// Parse a [`WasmManifestFile`] from a TOML string.
    ///
    /// # Errors
    /// Returns [`SwarmError::Internal`] if the TOML is invalid.
    pub fn from_toml_str(content: &str) -> SwarmResult<Self> {
        toml::from_str(content).map_err(|e| SwarmError::Internal {
            reason: format!("invalid WASM plugin manifest TOML: {e}"),
        })
    }

    /// Load a [`WasmManifestFile`] from a TOML file on disk.
    ///
    /// # Errors
    /// Returns [`SwarmError::Io`] on read failure or [`SwarmError::Internal`]
    /// on parse failure.
    pub fn load(path: &Path) -> SwarmResult<Self> {
        let content = std::fs::read_to_string(path).map_err(SwarmError::Io)?;
        Self::from_toml_str(&content)
    }

    /// Convert to a [`PluginManifest`] and the WASM binary path obtained by
    /// joining `manifest_dir` with the manifest's relative `wasm_file`.
    ///
    /// # Errors
    /// Returns [`SwarmError::Internal`] if the resolved WASM path does not
    /// exist.
    pub fn into_plugin_manifest_and_wasm_path(
        self,
        manifest_dir: &Path,
    ) -> SwarmResult<(PluginManifest, PathBuf)> {
        let wasm_path = manifest_dir.join(&self.plugin.wasm_file);

        if !wasm_path.exists() {
            return Err(SwarmError::Internal {
                reason: format!(
                    "WASM binary not found at '{}' (resolved from manifest)",
                    wasm_path.display()
                ),
            });
        }

        let manifest = self.into_plugin_manifest();
        Ok((manifest, wasm_path))
    }

    /// Convert to a [`PluginManifest`] without resolving the WASM path.
    ///
    /// This is useful for inspecting the manifest without loading the binary.
    pub fn into_plugin_manifest(self) -> PluginManifest {
        let p = self.plugin;
        PluginManifest {
            id: p.id.unwrap_or_else(PluginId::new),
            name: p.name,
            version: p.version,
            author: p.author,
            description: p.description,
            min_host_version: p.min_host_version,
            capabilities: p.capabilities,
            actions: p
                .actions
                .into_iter()
                .map(|a| PluginAction {
                    name: a.name,
                    description: a.description,
                    input_schema: a.input_schema,
                    output_schema: a.output_schema,
                })
                .collect(),
            required_permissions: p.required_permissions,
            wasm_permissions: p.wasm_permissions,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_TOML: &str = r#"
[plugin]
name    = "Test Plugin"
version = "0.1.0"
author  = "tester"
description = "A minimal test plugin"
wasm_file = "test.wasm"
"#;

    const FULL_TOML: &str = r#"
[plugin]
name             = "Full Plugin"
version          = "1.2.3"
author           = "Acme Corp"
description      = "All fields set"
min_host_version = "0.1.0"
wasm_file        = "full.wasm"
capabilities     = ["ActionProvider", "CommunicationChannel"]
required_permissions = ["read:config", "create:task"]

[[plugin.wasm_permissions]]
kind  = "Network"
value = "api.example.com:443"

[[plugin.wasm_permissions]]
kind  = "EnvVar"
value = "MY_API_KEY"

[[plugin.wasm_permissions]]
kind  = "FileRead"
value = "/etc/ssl/certs"

[[plugin.actions]]
name        = "send"
description = "Sends a message"

[[plugin.actions]]
name        = "receive"
description = "Receives a message"
"#;

    #[test]
    fn parse_minimal_manifest() {
        let m = WasmManifestFile::from_toml_str(MINIMAL_TOML).expect("should parse");
        assert_eq!(m.plugin.name, "Test Plugin");
        assert_eq!(m.plugin.version, "0.1.0");
        assert_eq!(m.plugin.wasm_file, PathBuf::from("test.wasm"));
        assert!(m.plugin.capabilities.is_empty());
        assert!(m.plugin.wasm_permissions.is_empty());
        // Default min_host_version is applied
        assert_eq!(m.plugin.min_host_version, "0.1.0");
    }

    #[test]
    fn parse_full_manifest() {
        let m = WasmManifestFile::from_toml_str(FULL_TOML).expect("should parse");
        assert_eq!(m.plugin.name, "Full Plugin");
        assert_eq!(m.plugin.capabilities.len(), 2);
        assert_eq!(m.plugin.actions.len(), 2);
        assert_eq!(m.plugin.required_permissions, vec!["read:config", "create:task"]);
        assert_eq!(m.plugin.wasm_permissions.len(), 3);
        assert_eq!(
            m.plugin.wasm_permissions[0],
            WasmPermission::Network("api.example.com:443".into())
        );
        assert_eq!(
            m.plugin.wasm_permissions[1],
            WasmPermission::EnvVar("MY_API_KEY".into())
        );
        assert_eq!(
            m.plugin.wasm_permissions[2],
            WasmPermission::FileRead("/etc/ssl/certs".into())
        );
    }

    #[test]
    fn into_plugin_manifest_conversion() {
        let m = WasmManifestFile::from_toml_str(FULL_TOML).expect("should parse");
        let manifest = m.into_plugin_manifest();
        assert_eq!(manifest.name, "Full Plugin");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.capabilities.len(), 2);
        assert_eq!(manifest.actions.len(), 2);
        assert_eq!(manifest.required_permissions.len(), 2);
        assert_eq!(manifest.wasm_permissions.len(), 3);
    }

    #[test]
    fn invalid_toml_returns_error() {
        let result = WasmManifestFile::from_toml_str("not valid toml [[[");
        assert!(result.is_err());
    }

    #[test]
    fn missing_required_field_returns_error() {
        // Missing `wasm_file`
        let toml = r#"
[plugin]
name = "No wasm_file"
version = "1.0.0"
author = "x"
description = "y"
"#;
        let result = WasmManifestFile::from_toml_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn wasm_permissions_serialise_and_deserialise() {
        let toml = r#"
[plugin]
name = "Perms"
version = "1.0.0"
author = "x"
description = "y"
wasm_file = "p.wasm"

[[plugin.wasm_permissions]]
kind  = "Network"
value = "localhost:8080"

[[plugin.wasm_permissions]]
kind  = "FileWrite"
value = "/tmp/out"

[[plugin.wasm_permissions]]
kind  = "Custom"
value = "special-feature"
"#;
        let m = WasmManifestFile::from_toml_str(toml).expect("should parse");
        assert_eq!(m.plugin.wasm_permissions, vec![
            WasmPermission::Network("localhost:8080".into()),
            WasmPermission::FileWrite("/tmp/out".into()),
            WasmPermission::Custom("special-feature".into()),
        ]);
    }
}
