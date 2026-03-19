//! Plugin lifecycle state machine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The current state of a plugin within the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginState {
    /// Plugin has been discovered but not yet loaded.
    Discovered,
    /// Plugin is being initialized (`on_load` is running).
    Loading,
    /// Plugin is loaded and healthy.
    Active,
    /// Plugin is being shut down.
    Unloading,
    /// Plugin has been cleanly unloaded.
    Unloaded,
    /// Plugin encountered an error.
    Failed {
        /// Description of the failure.
        reason: String,
        /// When the failure occurred.
        failed_at: DateTime<Utc>,
    },
}

impl PluginState {
    /// Returns `true` if the plugin is in a state where it can process requests.
    pub fn is_active(&self) -> bool {
        matches!(self, PluginState::Active)
    }

    /// Returns a short status label.
    pub fn label(&self) -> &'static str {
        match self {
            PluginState::Discovered => "discovered",
            PluginState::Loading => "loading",
            PluginState::Active => "active",
            PluginState::Unloading => "unloading",
            PluginState::Unloaded => "unloaded",
            PluginState::Failed { .. } => "failed",
        }
    }
}

/// Events emitted during plugin lifecycle transitions.
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub enum PluginLifecycleEvent {
    /// Plugin started loading.
    Loading {
        plugin_id: swarm_core::identity::PluginId,
    },
    /// Plugin loaded successfully and is now active.
    Activated {
        plugin_id: swarm_core::identity::PluginId,
    },
    /// Plugin failed to load or encountered a runtime error.
    Failed {
        plugin_id: swarm_core::identity::PluginId,
        reason: String,
    },
    /// Plugin was cleanly unloaded.
    Unloaded {
        plugin_id: swarm_core::identity::PluginId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_state_labels() {
        assert_eq!(PluginState::Active.label(), "active");
        assert_eq!(
            PluginState::Failed {
                reason: "err".into(),
                failed_at: Utc::now()
            }
            .label(),
            "failed"
        );
    }

    #[test]
    fn plugin_state_is_active() {
        assert!(PluginState::Active.is_active());
        assert!(!PluginState::Loading.is_active());
    }
}
