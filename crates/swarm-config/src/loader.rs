//! Configuration loader: reads TOML files and merges environment overrides.

use std::path::Path;

use swarm_core::error::{SwarmError, SwarmResult};

use crate::model::SwarmConfig;

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Loads [`SwarmConfig`] from one or more sources and merges them.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a TOML file.
    ///
    /// Missing optional keys fall back to their `Default` implementations.
    pub fn from_file(path: impl AsRef<Path>) -> SwarmResult<SwarmConfig> {
        let content =
            std::fs::read_to_string(path.as_ref()).map_err(|e| SwarmError::ConfigInvalid {
                key: path.as_ref().display().to_string(),
                reason: e.to_string(),
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
    /// - `SWARM_ROLES_DIR`
    /// - `SWARM_PROVIDER_DEFAULT_PROVIDER`
    /// - `SWARM_PROVIDER_DEFAULT_MODEL`
    /// - `SWARM_PROVIDER_ROUTING_STRATEGY`
    /// - `SWARM_MEMORY_BACKEND`
    /// - `SWARM_MEMORY_AUTO_APPLY_RETENTION`
    /// - `SWARM_LEARNING_ENABLED`
    /// - `SWARM_LEARNING_REQUIRE_APPROVAL_BY_DEFAULT`
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
        if let Ok(roles_dir) = std::env::var("SWARM_ROLES_DIR") {
            config.roles.roles_dir = Some(roles_dir);
        }
        if let Ok(provider) = std::env::var("SWARM_PROVIDER_DEFAULT_PROVIDER") {
            config.providers.default_provider = Some(provider);
        }
        if let Ok(model) = std::env::var("SWARM_PROVIDER_DEFAULT_MODEL") {
            config.providers.default_model = Some(model);
        }
        if let Ok(strategy) = std::env::var("SWARM_PROVIDER_ROUTING_STRATEGY") {
            config.providers.routing.strategy = match strategy.trim().to_ascii_lowercase().as_str()
            {
                "capability-match" | "capability_match" | "capability" => {
                    crate::model::ProviderRoutingStrategy::CapabilityMatch
                }
                "lowest-cost" | "lowest_cost" | "cost" => {
                    crate::model::ProviderRoutingStrategy::LowestCost
                }
                "lowest-latency" | "lowest_latency" | "latency" => {
                    crate::model::ProviderRoutingStrategy::LowestLatency
                }
                "round-robin" | "round_robin" | "rr" => {
                    crate::model::ProviderRoutingStrategy::RoundRobin
                }
                _ => config.providers.routing.strategy,
            };
        }
        if let Ok(backend) = std::env::var("SWARM_MEMORY_BACKEND") {
            config.memory.backend = match backend.trim().to_ascii_lowercase().as_str() {
                "in-memory" | "in_memory" | "memory" => crate::model::MemoryBackendKind::InMemory,
                "plugin" => crate::model::MemoryBackendKind::Plugin,
                _ => config.memory.backend,
            };
        }
        if let Ok(auto_apply) = std::env::var("SWARM_MEMORY_AUTO_APPLY_RETENTION") {
            if let Some(value) = parse_bool_env(&auto_apply) {
                config.memory.auto_apply_retention = value;
            }
        }
        if let Ok(enabled) = std::env::var("SWARM_LEARNING_ENABLED") {
            if let Some(value) = parse_bool_env(&enabled) {
                config.learning.enabled = value;
            }
        }
        if let Ok(approval) = std::env::var("SWARM_LEARNING_REQUIRE_APPROVAL_BY_DEFAULT") {
            if let Some(value) = parse_bool_env(&approval) {
                config.learning.require_approval_by_default = value;
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
        assert!(cfg.providers.enabled);
        assert_eq!(cfg.memory.retention_interval_secs, 300);
        assert!(cfg.learning.require_approval_by_default);
    }

    #[test]
    fn from_toml_str_invalid_fails() {
        let result = ConfigLoader::from_toml_str("not valid toml ::::");
        assert!(result.is_err());
    }

    #[test]
    fn env_overrides_apply_extended_settings() {
        std::env::set_var("SWARM_PROVIDER_DEFAULT_PROVIDER", "openai");
        std::env::set_var("SWARM_PROVIDER_ROUTING_STRATEGY", "round-robin");
        std::env::set_var("SWARM_MEMORY_BACKEND", "plugin");
        std::env::set_var("SWARM_MEMORY_AUTO_APPLY_RETENTION", "false");
        std::env::set_var("SWARM_LEARNING_ENABLED", "true");
        std::env::set_var("SWARM_LEARNING_REQUIRE_APPROVAL_BY_DEFAULT", "false");

        let cfg = ConfigLoader::with_env_overrides(ConfigLoader::defaults());

        assert_eq!(cfg.providers.default_provider.as_deref(), Some("openai"));
        assert_eq!(
            cfg.providers.routing.strategy,
            crate::model::ProviderRoutingStrategy::RoundRobin
        );
        assert_eq!(cfg.memory.backend, crate::model::MemoryBackendKind::Plugin);
        assert!(!cfg.memory.auto_apply_retention);
        assert!(cfg.learning.enabled);
        assert!(!cfg.learning.require_approval_by_default);

        std::env::remove_var("SWARM_PROVIDER_DEFAULT_PROVIDER");
        std::env::remove_var("SWARM_PROVIDER_ROUTING_STRATEGY");
        std::env::remove_var("SWARM_MEMORY_BACKEND");
        std::env::remove_var("SWARM_MEMORY_AUTO_APPLY_RETENTION");
        std::env::remove_var("SWARM_LEARNING_ENABLED");
        std::env::remove_var("SWARM_LEARNING_REQUIRE_APPROVAL_BY_DEFAULT");
    }
}
