//! Plugin host: manages plugin loading, unloading, and invocation.
//!
//! The [`PluginHost`] is the runtime container for plugins. It owns all loaded
//! plugin instances and routes invocations to the correct plugin.

use chrono::Utc;
use dashmap::DashMap;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::Mutex;

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::PluginId,
};

use crate::{lifecycle::PluginState, manifest::PluginManifest, registry::PluginRegistry, Plugin};

/// Host-side permission policy applied during plugin loading.
///
/// By default the host remains permissive for backward compatibility. When
/// allow-lists are configured, plugin loading fails if a manifest requests a
/// framework or WASM permission that is not explicitly allowed.
#[derive(Debug, Clone, Default)]
pub struct PluginPermissionPolicy {
    allowed_framework_permissions: Option<HashSet<String>>,
    allowed_wasm_permissions: Option<HashSet<crate::manifest::WasmPermission>>,
}

impl PluginPermissionPolicy {
    /// Create a permissive policy that does not restrict plugin permissions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict framework permissions to the provided allow-list.
    pub fn with_allowed_framework_permissions<I, S>(mut self, permissions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_framework_permissions =
            Some(permissions.into_iter().map(Into::into).collect());
        self
    }

    /// Restrict WASM permissions to the provided allow-list.
    pub fn with_allowed_wasm_permissions<I>(mut self, permissions: I) -> Self
    where
        I: IntoIterator<Item = crate::manifest::WasmPermission>,
    {
        self.allowed_wasm_permissions = Some(permissions.into_iter().collect());
        self
    }

    fn allows_framework_permission(&self, permission: &str) -> bool {
        self.allowed_framework_permissions
            .as_ref()
            .map_or(true, |allowed| allowed.contains(permission))
    }

    fn allows_wasm_permission(&self, permission: &crate::manifest::WasmPermission) -> bool {
        self.allowed_wasm_permissions
            .as_ref()
            .map_or(true, |allowed| allowed.contains(permission))
    }
}

/// Host-side invocation policy applied during plugin action execution.
///
/// By default the host remains permissive for backward compatibility. When a
/// grant list is configured, plugin invocation fails unless every declared
/// framework permission required by the plugin has been granted by the host.
#[derive(Debug, Clone, Default)]
pub struct PluginInvocationPolicy {
    granted_framework_permissions: Option<HashSet<String>>,
}

impl PluginInvocationPolicy {
    /// Create a permissive invocation policy that does not restrict runtime
    /// plugin permissions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant the provided framework permissions for plugin invocation.
    pub fn with_granted_framework_permissions<I, S>(mut self, permissions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.granted_framework_permissions =
            Some(permissions.into_iter().map(Into::into).collect());
        self
    }

    fn allows_framework_permission(&self, permission: &str) -> bool {
        self.granted_framework_permissions
            .as_ref()
            .map_or(true, |granted| granted.contains(permission))
    }

    fn missing_framework_permission<'a>(&self, manifest: &'a PluginManifest) -> Option<&'a str> {
        manifest
            .required_permissions
            .iter()
            .find(|permission| !self.allows_framework_permission(permission))
            .map(String::as_str)
    }
}

/// Runtime container for loaded plugins.
///
/// The host is responsible for:
/// 1. Loading plugins (calling [`Plugin::on_load`]).
/// 2. Routing invocations to the correct plugin.
/// 3. Updating lifecycle state during load and unload operations. Action-level
///    invocation errors are reported to the caller but do not automatically
///    transition the plugin to [`PluginState::Failed`].
/// 4. Unloading plugins gracefully.
pub struct PluginHost {
    registry: PluginRegistry,
    /// The live plugin instances (boxed trait objects).
    instances: DashMap<PluginId, Arc<Mutex<Box<dyn Plugin>>>>,
    permission_policy: PluginPermissionPolicy,
    invocation_policy: PluginInvocationPolicy,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            registry: PluginRegistry::default(),
            instances: DashMap::new(),
            permission_policy: PluginPermissionPolicy::default(),
            invocation_policy: PluginInvocationPolicy::default(),
        }
    }
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

    /// Create a plugin host with an explicit permission policy.
    pub fn with_permission_policy(permission_policy: PluginPermissionPolicy) -> Self {
        Self {
            permission_policy,
            ..Self::default()
        }
    }

    /// Create a plugin host with an explicit invocation policy.
    pub fn with_invocation_policy(invocation_policy: PluginInvocationPolicy) -> Self {
        Self {
            invocation_policy,
            ..Self::default()
        }
    }

    /// Create a plugin host with explicit load-time and invocation-time
    /// policies.
    pub fn with_policies(
        permission_policy: PluginPermissionPolicy,
        invocation_policy: PluginInvocationPolicy,
    ) -> Self {
        Self {
            permission_policy,
            invocation_policy,
            ..Self::default()
        }
    }

    fn validate_manifest_permissions(
        &self,
        manifest: &crate::manifest::PluginManifest,
    ) -> SwarmResult<()> {
        for permission in &manifest.required_permissions {
            if !self
                .permission_policy
                .allows_framework_permission(permission)
            {
                return Err(SwarmError::PermissionDenied {
                    subject: manifest.name.clone(),
                    permission: permission.clone(),
                    resource: "plugin_host.load".into(),
                });
            }
        }

        for permission in &manifest.wasm_permissions {
            if !self.permission_policy.allows_wasm_permission(permission) {
                return Err(SwarmError::PermissionDenied {
                    subject: manifest.name.clone(),
                    permission: permission.compact_string(),
                    resource: "plugin_host.load".into(),
                });
            }
        }

        Ok(())
    }

    fn resolve_declared_action<'a>(
        &self,
        manifest: &'a PluginManifest,
        action: &str,
    ) -> SwarmResult<&'a str> {
        manifest
            .actions
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(action))
            .map(|candidate| candidate.name.as_str())
            .ok_or_else(|| SwarmError::PluginOperationFailed {
                name: manifest.name.clone(),
                reason: format!("action '{action}' is not declared by the plugin manifest"),
            })
    }

    fn validate_invocation_permissions(
        &self,
        manifest: &PluginManifest,
        action: &str,
    ) -> SwarmResult<()> {
        if let Some(permission) = self
            .invocation_policy
            .missing_framework_permission(manifest)
        {
            return Err(SwarmError::PermissionDenied {
                subject: manifest.name.clone(),
                permission: permission.to_string(),
                resource: format!("plugin.invoke:{action}"),
            });
        }

        Ok(())
    }

    /// Load a plugin: register it, call `on_load`, and mark it active.
    pub async fn load(&self, mut plugin: Box<dyn Plugin>) -> SwarmResult<PluginId> {
        let manifest = plugin.manifest().clone();
        manifest.validate_for_host(env!("CARGO_PKG_VERSION"))?;
        self.validate_manifest_permissions(&manifest)?;
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
        let record =
            self.registry
                .get(plugin_id)
                .ok_or_else(|| SwarmError::PluginOperationFailed {
                    name: plugin_id.to_string(),
                    reason: "plugin not loaded".into(),
                })?;
        let declared_action = self.resolve_declared_action(&record.manifest, action)?;
        self.validate_invocation_permissions(&record.manifest, declared_action)?;
        let state = record.state;

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
        plugin.invoke(declared_action, params).await.map_err(|e| {
            SwarmError::PluginOperationFailed {
                name,
                reason: e.to_string(),
            }
        })
    }

    /// Unload a plugin gracefully.
    pub async fn unload(&self, plugin_id: &PluginId) -> SwarmResult<()> {
        let instance = self
            .instances
            .get(plugin_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| SwarmError::PluginOperationFailed {
                name: plugin_id.to_string(),
                reason: "plugin not loaded".into(),
            })?;

        self.registry
            .update_state(plugin_id, PluginState::Unloading)?;
        let mut plugin = instance.lock().await;
        let name = plugin.manifest().name.clone();

        match plugin.on_unload().await {
            Ok(()) => {
                self.instances.remove(plugin_id);
                self.registry
                    .update_state(plugin_id, PluginState::Unloaded)?;
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
                self.registry
                    .update_state(plugin_id, PluginState::Unloaded)?;
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
    use crate::manifest::{PluginAction, PluginCapabilityKind, PluginManifest, WasmPermission};
    use async_trait::async_trait;

    struct EchoPlugin {
        manifest: PluginManifest,
    }

    struct FailingLoadPlugin {
        manifest: PluginManifest,
    }

    struct InvalidManifestPlugin {
        manifest: PluginManifest,
    }

    impl EchoPlugin {
        fn new() -> Self {
            let mut manifest = PluginManifest::new("echo", "1.0.0", "test", "Echoes inputs");
            manifest
                .capabilities
                .push(PluginCapabilityKind::ActionProvider);
            manifest.actions.push(PluginAction {
                name: "echo".into(),
                description: "Echo the provided parameters".into(),
                input_schema: None,
                output_schema: None,
            });
            Self { manifest }
        }

        fn with_permissions(
            required_permissions: &[&str],
            wasm_permissions: &[WasmPermission],
        ) -> Self {
            let mut plugin = Self::new();
            plugin.manifest.required_permissions = required_permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect();
            plugin.manifest.wasm_permissions = wasm_permissions.to_vec();
            plugin
        }
    }

    impl FailingLoadPlugin {
        fn new() -> Self {
            let mut manifest =
                PluginManifest::new("failing-load", "1.0.0", "test", "Fails during load");
            manifest
                .capabilities
                .push(PluginCapabilityKind::ActionProvider);
            manifest.actions.push(PluginAction {
                name: "fail".into(),
                description: "Always fails during load".into(),
                input_schema: None,
                output_schema: None,
            });
            Self { manifest }
        }
    }

    impl InvalidManifestPlugin {
        fn new() -> Self {
            let mut manifest = PluginManifest::new(
                "invalid-manifest",
                "1.0.0",
                "test",
                "Has invalid manifest metadata",
            );
            manifest
                .required_permissions
                .push("invalid-permission".into());
            Self { manifest }
        }
    }

    #[async_trait]
    impl Plugin for EchoPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        async fn on_load(&mut self) -> SwarmResult<()> {
            Ok(())
        }
        async fn on_unload(&mut self) -> SwarmResult<()> {
            Ok(())
        }
        async fn invoke(
            &mut self,
            _action: &str,
            params: serde_json::Value,
        ) -> SwarmResult<serde_json::Value> {
            Ok(params)
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Plugin for FailingLoadPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        async fn on_load(&mut self) -> SwarmResult<()> {
            Err(SwarmError::PluginInitFailed {
                name: self.manifest.name.clone(),
                reason: "load failed".into(),
            })
        }
        async fn on_unload(&mut self) -> SwarmResult<()> {
            Ok(())
        }
        async fn invoke(
            &mut self,
            _action: &str,
            _params: serde_json::Value,
        ) -> SwarmResult<serde_json::Value> {
            Ok(serde_json::Value::Null)
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Plugin for InvalidManifestPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        async fn on_load(&mut self) -> SwarmResult<()> {
            Ok(())
        }
        async fn on_unload(&mut self) -> SwarmResult<()> {
            Ok(())
        }
        async fn invoke(
            &mut self,
            _action: &str,
            _params: serde_json::Value,
        ) -> SwarmResult<serde_json::Value> {
            Ok(serde_json::Value::Null)
        }
        async fn health_check(&self) -> SwarmResult<()> {
            Ok(())
        }
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
        let record = host
            .registry()
            .get(&plugin_id)
            .expect("failed load should remain visible");
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
        assert!(matches!(
            first_result,
            Err(SwarmError::PluginInitFailed { .. })
        ));

        let mut echo_plugin = EchoPlugin::new();
        echo_plugin.manifest.id = plugin_id;
        let second_result = host.load(Box::new(echo_plugin)).await;

        assert_eq!(second_result.unwrap(), plugin_id);
        assert_eq!(
            host.registry()
                .get(&plugin_id)
                .expect("plugin should be reloaded")
                .state
                .label(),
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
                .invoke(
                    &invoke_plugin_id,
                    "echo",
                    serde_json::json!({"msg": "hello"}),
                )
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
        assert_eq!(
            host.registry().get(&plugin_id).unwrap().state.label(),
            "unloaded"
        );
    }

    #[tokio::test]
    async fn load_rejects_invalid_manifest_before_registration() {
        let host = PluginHost::new();

        let error = host
            .load(Box::new(InvalidManifestPlugin::new()))
            .await
            .expect_err("invalid manifest should be rejected");

        assert!(matches!(error, SwarmError::ConfigInvalid { .. }));
        assert!(host.registry().all().is_empty());
    }

    #[tokio::test]
    async fn load_allows_manifest_permissions_by_default() {
        let host = PluginHost::new();

        let plugin_id = host
            .load(Box::new(EchoPlugin::with_permissions(
                &["read:config"],
                &[WasmPermission::EnvVar("API_TOKEN".into())],
            )))
            .await
            .unwrap();

        assert_eq!(
            host.registry().get(&plugin_id).unwrap().state.label(),
            "active"
        );
    }

    #[tokio::test]
    async fn load_rejects_framework_permissions_outside_allowlist() {
        let host = PluginHost::with_permission_policy(
            PluginPermissionPolicy::new().with_allowed_framework_permissions(["read:config"]),
        );

        let error = host
            .load(Box::new(EchoPlugin::with_permissions(
                &["create:task"],
                &[],
            )))
            .await
            .expect_err("unexpected framework permission should be denied");

        assert!(matches!(
            error,
            SwarmError::PermissionDenied { permission, .. } if permission == "create:task"
        ));
        assert!(host.registry().all().is_empty());
    }

    #[tokio::test]
    async fn load_rejects_wasm_permissions_outside_allowlist() {
        let host = PluginHost::with_permission_policy(
            PluginPermissionPolicy::new()
                .with_allowed_wasm_permissions([WasmPermission::EnvVar("SAFE_TOKEN".into())]),
        );

        let error = host
            .load(Box::new(EchoPlugin::with_permissions(
                &[],
                &[WasmPermission::Network("api.example.com:443".into())],
            )))
            .await
            .expect_err("unexpected wasm permission should be denied");

        assert!(matches!(
            error,
            SwarmError::PermissionDenied { permission, .. }
                if permission == "network:api.example.com:443"
        ));
        assert!(host.registry().all().is_empty());
    }

    #[tokio::test]
    async fn load_accepts_permissions_inside_allowlists() {
        let host = PluginHost::with_permission_policy(
            PluginPermissionPolicy::new()
                .with_allowed_framework_permissions(["read:config", "create:task"])
                .with_allowed_wasm_permissions([
                    WasmPermission::EnvVar("API_TOKEN".into()),
                    WasmPermission::Network("api.example.com:443".into()),
                ]),
        );

        let plugin_id = host
            .load(Box::new(EchoPlugin::with_permissions(
                &["read:config"],
                &[
                    WasmPermission::EnvVar("API_TOKEN".into()),
                    WasmPermission::Network("api.example.com:443".into()),
                ],
            )))
            .await
            .unwrap();

        assert_eq!(
            host.registry().get(&plugin_id).unwrap().state.label(),
            "active"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_actions_missing_from_manifest() {
        let host = PluginHost::new();
        let plugin_id = host.load(Box::new(EchoPlugin::new())).await.unwrap();

        let error = host
            .invoke(
                &plugin_id,
                "missing-action",
                serde_json::json!({"msg": "hello"}),
            )
            .await
            .expect_err("undeclared actions should be rejected");

        assert!(matches!(
            error,
            SwarmError::PluginOperationFailed { reason, .. }
                if reason == "action 'missing-action' is not declared by the plugin manifest"
        ));
    }

    #[tokio::test]
    async fn invoke_rejects_runtime_permissions_outside_grant_list() {
        let host = PluginHost::with_invocation_policy(
            PluginInvocationPolicy::new().with_granted_framework_permissions(["read:config"]),
        );
        let plugin_id = host
            .load(Box::new(EchoPlugin::with_permissions(
                &["create:task"],
                &[],
            )))
            .await
            .unwrap();

        let error = host
            .invoke(&plugin_id, "echo", serde_json::json!({"msg": "hello"}))
            .await
            .expect_err("missing invocation grants should be denied");

        assert!(matches!(
            error,
            SwarmError::PermissionDenied { permission, resource, .. }
                if permission == "create:task" && resource == "plugin.invoke:echo"
        ));
    }

    #[tokio::test]
    async fn invoke_accepts_runtime_permissions_inside_grant_list() {
        let host = PluginHost::with_invocation_policy(
            PluginInvocationPolicy::new().with_granted_framework_permissions(["read:config"]),
        );
        let plugin_id = host
            .load(Box::new(EchoPlugin::with_permissions(
                &["read:config"],
                &[],
            )))
            .await
            .unwrap();

        let result = host
            .invoke(&plugin_id, "ECHO", serde_json::json!({"msg": "hello"}))
            .await
            .unwrap();

        assert_eq!(result, serde_json::json!({"msg": "hello"}));
    }
}
