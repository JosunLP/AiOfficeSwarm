//! # swarm-plugin
//!
//! Plugin SDK for the AiOfficeSwarm framework.
//!
//! This crate defines the contract between the framework host and third-party
//! plugins. Plugin authors implement the [`Plugin`] trait and describe their
//! plugin via a [`PluginManifest`].
//!
//! ## Plugin types
//! Plugins can provide one or more of the following capabilities:
//!
//! - **AgentProvider**: registers new agent types with the orchestrator.
//! - **ActionProvider**: adds new callable actions to agents.
//! - **StorageBackend**: plugs in alternative persistence layers.
//! - **CommunicationChannel**: connects external messaging systems
//!   (Teams, Slack, email, etc.).
//! - **PolicyProvider**: contributes new policy rules.
//! - **TriggerProvider**: reacts to external events and submits tasks.
//!
//! ## WASM plugins (`wasm` feature, enabled by default)
//!
//! When the `wasm` feature is active (the default), two additional modules are
//! available:
//!
//! - [`wasm_manifest`] – types for the on-disk TOML manifest format and the
//!   [`wasm_manifest::WasmManifestFile`] loader.
//! - [`wasm_loader`] – [`wasm_loader::WasmPlugin`] (implements [`Plugin`] via
//!   the *wasmtime* runtime) and [`wasm_loader::WasmPluginLoader`] (high-level
//!   file-based loader).
//!
//! ## Security
//! All plugin invocations pass through the policy engine. Plugins declare
//! their required permissions in the manifest; the host validates these
//! against the RBAC configuration before loading the plugin. WASM plugins
//! additionally declare OS-level sandbox permissions via
//! [`manifest::WasmPermission`].

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod host;
pub mod lifecycle;
pub mod manifest;
pub mod registry;

#[cfg(feature = "wasm")]
pub mod wasm_manifest;

#[cfg(feature = "wasm")]
pub mod wasm_loader;

pub use host::PluginHost;
pub use lifecycle::{PluginLifecycleEvent, PluginState};
pub use manifest::{PluginCapabilityKind, PluginManifest, WasmPermission};
pub use registry::PluginRegistry;

use async_trait::async_trait;
use swarm_core::error::SwarmResult;

/// The primary trait that every plugin must implement.
///
/// Implement this trait in your plugin crate, then register the plugin with
/// the [`PluginHost`] via [`PluginHost::load`].
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Returns the static manifest describing this plugin.
    fn manifest(&self) -> &PluginManifest;

    /// Called once after the plugin is loaded. Use for one-time initialization
    /// (e.g., connecting to external systems, loading config).
    async fn on_load(&mut self) -> SwarmResult<()>;

    /// Called once before the plugin is unloaded. Use for graceful shutdown.
    async fn on_unload(&mut self) -> SwarmResult<()>;

    /// Invoke a named action provided by this plugin.
    ///
    /// The `action` parameter matches one of the action names declared in the
    /// manifest. The `params` are free-form JSON.
    async fn invoke(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> SwarmResult<serde_json::Value>;

    /// Perform a health check. Return `Ok(())` if the plugin is healthy.
    async fn health_check(&self) -> SwarmResult<()>;
}
