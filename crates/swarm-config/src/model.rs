//! Top-level configuration model.
//!
//! All settings for the framework are captured in [`SwarmConfig`]. The struct
//! derives `serde` so it can be deserialized from TOML, JSON, or environment
//! variables.

use serde::{Deserialize, Serialize};

/// Top-level framework configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SwarmConfig {
    /// Human-readable name for this swarm instance.
    pub instance_name: String,
    /// Orchestrator-specific settings.
    pub orchestrator: OrchestratorConfig,
    /// Telemetry (logging and metrics) settings.
    pub telemetry: TelemetryConfig,
    /// Plugin loading settings.
    pub plugins: PluginsConfig,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            instance_name: "ai-office-swarm".into(),
            orchestrator: OrchestratorConfig::default(),
            telemetry: TelemetryConfig::default(),
            plugins: PluginsConfig::default(),
        }
    }
}

/// Orchestrator-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestratorConfig {
    /// Capacity of the internal event broadcast channel.
    pub event_channel_capacity: usize,
    /// How many task dispatch attempts per scheduling tick.
    pub max_dispatch_per_tick: usize,
    /// Reserved for future runtime enforcement: default task timeout in seconds
    /// when a task spec leaves its timeout as `None` (0 = no timeout).
    /// `TaskSpec::new` currently sets its own five-minute timeout explicitly.
    pub default_task_timeout_secs: u64,
    /// Reserved for future scheduler/runtime enforcement: maximum number of
    /// concurrent tasks across the whole swarm.
    pub max_concurrent_tasks: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            event_channel_capacity: 1024,
            max_dispatch_per_tick: 16,
            default_task_timeout_secs: 300,
            max_concurrent_tasks: 256,
        }
    }
}

/// Telemetry (tracing and metrics) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    /// Tracing log level (e.g., `"info"`, `"debug"`, `"warn"`).
    pub log_level: String,
    /// Log format: `"text"` or `"json"`.
    pub log_format: LogFormat,
    /// Whether to emit OpenTelemetry traces.
    pub otlp_enabled: bool,
    /// OTLP endpoint URL.
    pub otlp_endpoint: Option<String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            log_format: LogFormat::Text,
            otlp_enabled: false,
            otlp_endpoint: None,
        }
    }
}

/// Supported log output formats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text output.
    Text,
    /// Structured JSON output (suitable for log aggregators).
    Json,
}

/// Plugin subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PluginsConfig {
    /// Directory to scan for plugin definitions.
    pub plugin_dir: Option<String>,
    /// Whether to auto-load plugins found in `plugin_dir` on startup.
    pub auto_load: bool,
}
