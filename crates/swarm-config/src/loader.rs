//! Configuration loader: reads TOML files and merges environment overrides.

use std::path::Path;

use swarm_core::error::{SwarmError, SwarmResult};

use crate::model::SwarmConfig;

/// Loads [`SwarmConfig`] from one or more sources and merges them.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a TOML file.
    ///
    /// Missing optional keys fall back to their `Default` implementations.
    pub fn from_file(path: impl AsRef<Path>) -> SwarmResult<SwarmConfig> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            SwarmError::ConfigInvalid {
                key: path.as_ref().display().to_string(),
                reason: e.to_string(),
            }
        })?;
        Self::from_toml_str(&content)
    }

    /// Parse configuration from a TOML string.
    pub fn from_toml_str(content: &str) -> SwarmResult<SwarmConfig> {
        toml::from_str(content).map_err(|e| SwarmError::ConfigInvalid {
            key: "(document)".into(),
            reason: e.to_string(),
        })
    }

    /// Return the default configuration.
    pub fn defaults() -> SwarmConfig {
        SwarmConfig::default()
    }

    /// Load configuration with environment variable overrides applied.
    ///
    /// This is a best-effort layer: only explicitly recognized flat variables
    /// are applied today and unknown variables are silently ignored.
    ///
    /// Supported variables:
    /// - `SWARM_LOG_LEVEL`
    /// - `SWARM_INSTANCE_NAME`
    /// - `SWARM_EVENT_CHANNEL_CAPACITY`
    pub fn with_env_overrides(mut config: SwarmConfig) -> SwarmConfig {
        if let Ok(level) = std::env::var("SWARM_LOG_LEVEL") {
            config.telemetry.log_level = level;
        }
        if let Ok(name) = std::env::var("SWARM_INSTANCE_NAME") {
            config.instance_name = name;
        }
        if let Ok(cap) = std::env::var("SWARM_EVENT_CHANNEL_CAPACITY") {
            if let Ok(v) = cap.parse() {
                config.orchestrator.event_channel_capacity = v;
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = ConfigLoader::defaults();
        assert_eq!(cfg.instance_name, "ai-office-swarm");
        assert_eq!(cfg.orchestrator.event_channel_capacity, 1024);
    }

    #[test]
    fn from_toml_str_minimal() {
        let toml = r#"
            instance_name = "my-swarm"
            [orchestrator]
            max_dispatch_per_tick = 8
        "#;
        let cfg = ConfigLoader::from_toml_str(toml).unwrap();
        assert_eq!(cfg.instance_name, "my-swarm");
        assert_eq!(cfg.orchestrator.max_dispatch_per_tick, 8);
        // defaults should apply for omitted fields
        assert_eq!(cfg.orchestrator.event_channel_capacity, 1024);
    }

    #[test]
    fn from_toml_str_invalid_fails() {
        let result = ConfigLoader::from_toml_str("not valid toml ::::");
        assert!(result.is_err());
    }
}
