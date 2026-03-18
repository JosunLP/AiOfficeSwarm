//! Plugin host: manages plugin loading, unloading, and invocation.
//!
//! The [`PluginHost`] is the runtime container for plugins. It owns all loaded
//! plugin instances and routes invocations to the correct plugin.

use std::sync::Arc;
use tokio::sync::Mutex;
use dashmap::DashMap;
use chrono::Utc;

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::PluginId,
};

use crate::{
    lifecycle::PluginState,
    registry::PluginRegistry,
    Plugin,
};

/// Runtime container for loaded plugins.
///
/// The host is responsible for:
/// 1. Loading plugins (calling [`Plugin::on_load`]).
/// 2. Routing invocations to the correct plugin.
/// 3. Updating lifecycle state during load and unload operations. Action-level
///    invocation errors are reported to the caller but do not automatically
///    transition the plugin to [`PluginState::Failed`].
/// 4. Unloading plugins gracefully.
#[derive(Default)]
pub struct PluginHost {
    registry: PluginRegistry,
    /// The live plugin instances (boxed trait objects).
    instances: DashMap<PluginId, Arc<Mutex<Box<dyn Plugin>>>>,
}

impl PluginHost {
    fn prepare_registry_slot(&self, id: &PluginId) -> SwarmResult<()> {
        if let Some(existing) = self.registry.get(id) {
            match existing.state {
                PluginState::Failed { .. } => self.registry.deregister(id)?,
                _ => {
                    return Err(SwarmError::Internal {
                        reason: format!("plugin {} is already registered", id),
                    });
                }
            }
        }

        Ok(())
    }

    /// Create an empty plugin host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a plugin: register it, call `on_load`, and mark it active.
    pub async fn load(&self, mut plugin: Box<dyn Plugin>) -> SwarmResult<PluginId> {
        let manifest = plugin.manifest().clone();
        let id = manifest.id;
        let name = manifest.name.clone();

        self.prepare_registry_slot(&id)?;
        self.registry.register(manifest)?;
        self.registry.update_state(&id, PluginState::Loading)?;

        tracing::info!(plugin_id = %id, name = %name, "Loading plugin");

        match plugin.on_load().await {
            Ok(()) => {
                self.registry.update_state(&id, PluginState::Active)?;
                self.instances.insert(id, Arc::new(Mutex::new(plugin)));
                tracing::info!(plugin_id = %id, name = %name, "Plugin loaded and active");
                Ok(id)
            }
            Err(e) => {
                let reason = e.to_string();
                self.registry.update_state(
                    &id,
                    PluginState::Failed {
                        reason: reason.clone(),
                        failed_at: Utc::now(),
                    },
                )?;
                tracing::error!(plugin_id = %id, name = %name, reason = %reason, "Plugin failed to load");
                Err(SwarmError::PluginInitFailed { name, reason })
            }
        }
    }

    /// Invoke a named action on the specified plugin.
    pub async fn invoke(
        &self,
        plugin_id: &PluginId,
        action: &str,
        params: serde_json::Value,
    ) -> SwarmResult<serde_json::Value> {
        let state = self
            .registry
            .get(plugin_id)
            .ok_or_else(|| SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: "plugin not loaded".into(),
            })?
            .state;

        if !state.is_active() {
            return Err(SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: format!("plugin is {}", state.label()),
            });
        }

        let instance = self
            .instances
            .get(plugin_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: "plugin not loaded".into(),
            })?;

        let mut plugin = instance.lock().await;
        let state = self
            .registry
            .get(plugin_id)
            .ok_or_else(|| SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: "plugin not loaded".into(),
            })?
            .state;

        if !state.is_active() {
            return Err(SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: format!("plugin is {}", state.label()),
            });
        }

        let name = plugin.manifest().name.clone();
        plugin.invoke(action, params).await.map_err(|e| {
            SwarmError::PluginOperationFailed {
                name,
                reason: e.to_string(),
            }
        })
    }

    /// Unload a plugin gracefully.
    pub async fn unload(&self, plugin_id: &PluginId) -> SwarmResult<()> {
        let instance = self.instances.get(plugin_id).map(|entry| Arc::clone(entry.value())).ok_or_else(|| {
            SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: "plugin not loaded".into(),
            }
        })?;

        self.registry.update_state(plugin_id, PluginState::Unloading)?;
        let mut plugin = instance.lock().await;
        let name = plugin.manifest().name.clone();

        match plugin.on_unload().await {
            Ok(()) => {
                self.instances.remove(plugin_id);
                self.registry.update_state(plugin_id, PluginState::Unloaded)?;
                tracing::info!(plugin_id = %plugin_id, name = %name, "Plugin unloaded");
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    plugin_id = %plugin_id,
                    name = %name,
                    error = e.to_string(),
                    "Plugin unload produced an error (continuing)"
                );
                self.instances.remove(plugin_id);
                self.registry.update_state(plugin_id, PluginState::Unloaded)?;
                Ok(())
            }
        }
    }

    /// Return the registry for external inspection.
    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    /// Perform health checks on all active plugins and return a map of
    /// plugin ID → health result.
    pub async fn health_check_all(&self) -> Vec<(PluginId, SwarmResult<()>)> {
        let instances: Vec<_> = self
            .instances
            .iter()
            .map(|entry| (*entry.key(), Arc::clone(entry.value())))
            .collect();
        let mut results = Vec::new();
        for (id, instance) in instances {
            let plugin = instance.lock().await;
            let result = plugin.health_check().await;
            results.push((id, result));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PluginManifest, PluginCapabilityKind};
    use async_trait::async_trait;

    struct EchoPlugin {
        manifest: PluginManifest,
    }

    struct FailingLoadPlugin {
        manifest: PluginManifest,
    }

    impl EchoPlugin {
        fn new() -> Self {
            let mut manifest = PluginManifest::new("echo", "1.0.0", "test", "Echoes inputs");
            manifest.capabilities.push(PluginCapabilityKind::ActionProvider);
            Self { manifest }
        }
    }

    impl FailingLoadPlugin {
        fn new() -> Self {
            let mut manifest =
                PluginManifest::new("failing-load", "1.0.0", "test", "Fails during load");
            manifest.capabilities.push(PluginCapabilityKind::ActionProvider);
            Self { manifest }
        }
    }

    #[async_trait]
    impl Plugin for EchoPlugin {
        fn manifest(&self) -> &PluginManifest { &self.manifest }
        async fn on_load(&mut self) -> SwarmResult<()> { Ok(()) }
        async fn on_unload(&mut self) -> SwarmResult<()> { Ok(()) }
        async fn invoke(&mut self, _action: &str, params: serde_json::Value) -> SwarmResult<serde_json::Value> {
            Ok(params)
        }
        async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
    }

    #[async_trait]
    impl Plugin for FailingLoadPlugin {
        fn manifest(&self) -> &PluginManifest { &self.manifest }
        async fn on_load(&mut self) -> SwarmResult<()> {
            Err(SwarmError::PluginInitFailed {
                name: self.manifest.name.clone(),
                reason: "load failed".into(),
            })
        }
        async fn on_unload(&mut self) -> SwarmResult<()> { Ok(()) }
        async fn invoke(&mut self, _action: &str, _params: serde_json::Value) -> SwarmResult<serde_json::Value> {
            Ok(serde_json::Value::Null)
        }
        async fn health_check(&self) -> SwarmResult<()> { Ok(()) }
    }

    #[tokio::test]
    async fn load_invoke_unload() {
        let host = PluginHost::new();
        let plugin_id = host.load(Box::new(EchoPlugin::new())).await.unwrap();

        let result = host
            .invoke(&plugin_id, "echo", serde_json::json!({"msg": "hello"}))
            .await
            .unwrap();
        assert_eq!(result, serde_json::json!({"msg": "hello"}));

        host.unload(&plugin_id).await.unwrap();
        let record = host.registry().get(&plugin_id).unwrap();
        assert_eq!(record.state.label(), "unloaded");
    }

    #[tokio::test]
    async fn failed_load_keeps_failed_plugin_record() {
        let host = PluginHost::new();
        let plugin = FailingLoadPlugin::new();
        let plugin_id = plugin.manifest.id;

        let result = host.load(Box::new(plugin)).await;

        assert!(matches!(result, Err(SwarmError::PluginInitFailed { .. })));
        let record = host.registry().get(&plugin_id).expect("failed load should remain visible");
        assert_eq!(record.state.label(), "failed");
        assert!(host.registry().active_plugins().is_empty());
    }

    #[tokio::test]
    async fn failed_load_can_be_retried_with_same_plugin_id() {
        let host = PluginHost::new();
        let plugin_id = PluginId::new();

        let mut failing_plugin = FailingLoadPlugin::new();
        failing_plugin.manifest.id = plugin_id;
        let first_result = host.load(Box::new(failing_plugin)).await;
        assert!(matches!(first_result, Err(SwarmError::PluginInitFailed { .. })));

        let mut echo_plugin = EchoPlugin::new();
        echo_plugin.manifest.id = plugin_id;
        let second_result = host.load(Box::new(echo_plugin)).await;

        assert_eq!(second_result.unwrap(), plugin_id);
        assert_eq!(
            host.registry().get(&plugin_id).expect("plugin should be reloaded").state.label(),
            "active"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_once_unload_starts_even_if_instance_was_already_cloned() {
        let host = Arc::new(PluginHost::new());
        let plugin_id = host.load(Box::new(EchoPlugin::new())).await.unwrap();
        let instance = host
            .instances
            .get(&plugin_id)
            .map(|entry| Arc::clone(entry.value()))
            .expect("instance should exist after load");

        let guard = instance.lock().await;

        let invoke_host = Arc::clone(&host);
        let invoke_plugin_id = plugin_id;
        let invoke_task = tokio::spawn(async move {
            invoke_host
                .invoke(&invoke_plugin_id, "echo", serde_json::json!({"msg": "hello"}))
                .await
        });

        tokio::task::yield_now().await;

        let unload_host = Arc::clone(&host);
        let unload_plugin_id = plugin_id;
        let unload_task = tokio::spawn(async move { unload_host.unload(&unload_plugin_id).await });

        tokio::task::yield_now().await;
        drop(guard);

        let invoke_result = invoke_task.await.unwrap();
        assert!(matches!(
            invoke_result,
            Err(SwarmError::PluginOperationFailed { reason, .. }) if reason == "plugin is unloading"
        ));

        unload_task.await.unwrap().unwrap();
        assert_eq!(host.registry().get(&plugin_id).unwrap().state.label(), "unloaded");
    }
}
