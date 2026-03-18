//! # example-integration
//!
//! A demonstration plugin for the AiOfficeSwarm framework.
//!
//! This plugin illustrates how to implement the [`Plugin`] trait and expose
//! actions. It simulates a generic "Notification Service" integration with
//! two actions:
//!
//! - `send_notification`: Sends a notification to a configured channel.
//! - `get_status`: Returns the plugin's current connection status.
//!
//! ## Adding a real integration
//! Replace the mock implementations in [`NotificationPlugin`] with real
//! HTTP calls, SDK invocations, etc. The manifest, action declarations, and
//! plugin lifecycle are framework contracts and should remain stable.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

use async_trait::async_trait;
use serde_json::json;

use swarm_core::error::{SwarmError, SwarmResult};
use swarm_plugin::{
    manifest::{PluginAction, PluginCapabilityKind, PluginManifest},
    Plugin,
};

/// A mock notification service plugin.
///
/// In a real implementation this would hold a connection handle or HTTP client.
pub struct NotificationPlugin {
    manifest: PluginManifest,
    /// Simulated connection state.
    connected: bool,
    /// Configured channel name (loaded from config on startup).
    channel: String,
}

impl NotificationPlugin {
    /// Create a new plugin instance targeting the given channel.
    pub fn new(channel: impl Into<String>) -> Self {
        let mut manifest = PluginManifest::new(
            "Notification Service",
            "1.0.0",
            "AiOfficeSwarm Contributors",
            "Sends notifications to a configured channel",
        );
        manifest
            .capabilities
            .push(PluginCapabilityKind::ActionProvider);
        manifest
            .capabilities
            .push(PluginCapabilityKind::CommunicationChannel);
        manifest.actions.push(PluginAction {
            name: "send_notification".into(),
            description: "Send a notification message to the configured channel".into(),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "severity": { "type": "string", "enum": ["info", "warn", "error"] }
                },
                "required": ["message"]
            })),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "delivered": { "type": "boolean" },
                    "channel": { "type": "string" }
                }
            })),
        });
        manifest.actions.push(PluginAction {
            name: "get_status".into(),
            description: "Return the plugin's connection status".into(),
            input_schema: None,
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "connected": { "type": "boolean" },
                    "channel": { "type": "string" }
                }
            })),
        });

        Self {
            manifest,
            connected: false,
            channel: channel.into(),
        }
    }
}

#[async_trait]
impl Plugin for NotificationPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn on_load(&mut self) -> SwarmResult<()> {
        // In a real implementation: establish a connection.
        tracing::info!(
            plugin = %self.manifest.name,
            channel = %self.channel,
            "NotificationPlugin loaded; simulating connection to channel"
        );
        self.connected = true;
        Ok(())
    }

    async fn on_unload(&mut self) -> SwarmResult<()> {
        tracing::info!(plugin = %self.manifest.name, "NotificationPlugin unloading");
        self.connected = false;
        Ok(())
    }

    async fn invoke(
        &mut self,
        action: &str,
        params: serde_json::Value,
    ) -> SwarmResult<serde_json::Value> {
        if !self.connected {
            return Err(SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: "plugin is not connected".into(),
            });
        }

        match action {
            "send_notification" => {
                let message = params["message"].as_str().ok_or_else(|| {
                    SwarmError::PluginOperationFailed {
                        name: self.manifest.name.clone(),
                        reason: "missing 'message' parameter".into(),
                    }
                })?;
                let severity = params["severity"].as_str().unwrap_or("info");

                tracing::info!(
                    plugin = %self.manifest.name,
                    channel = %self.channel,
                    severity = severity,
                    message = message,
                    "Sending notification (mock)"
                );

                Ok(json!({
                    "delivered": true,
                    "channel": &self.channel,
                    "severity": severity,
                    "message": message,
                }))
            }
            "get_status" => Ok(json!({
                "connected": self.connected,
                "channel": &self.channel,
            })),
            other => Err(SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: format!("unknown action: '{}'", other),
            }),
        }
    }

    async fn health_check(&self) -> SwarmResult<()> {
        if self.connected {
            Ok(())
        } else {
            Err(SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: "not connected".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_plugin::PluginHost;

    #[tokio::test]
    async fn load_and_invoke_send_notification() {
        let host = PluginHost::new();
        let plugin_id = host
            .load(Box::new(NotificationPlugin::new("#alerts")))
            .await
            .expect("should load");

        let result = host
            .invoke(
                &plugin_id,
                "send_notification",
                json!({ "message": "Hello from the swarm!", "severity": "info" }),
            )
            .await
            .expect("should succeed");

        assert_eq!(result["delivered"], true);
        assert_eq!(result["channel"], "#alerts");
    }

    #[tokio::test]
    async fn get_status_returns_connected() {
        let host = PluginHost::new();
        let plugin_id = host
            .load(Box::new(NotificationPlugin::new("#general")))
            .await
            .unwrap();

        let status = host
            .invoke(&plugin_id, "get_status", json!({}))
            .await
            .unwrap();

        assert_eq!(status["connected"], true);
    }

    #[tokio::test]
    async fn unknown_action_returns_error() {
        let host = PluginHost::new();
        let plugin_id = host
            .load(Box::new(NotificationPlugin::new("#test")))
            .await
            .unwrap();

        let result = host.invoke(&plugin_id, "nonexistent", json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn health_check_passes_when_connected() {
        let host = PluginHost::new();
        let _plugin_id = host
            .load(Box::new(NotificationPlugin::new("#health")))
            .await
            .unwrap();

        let results = host.health_check_all().await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
    }
}
