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
    /// Provider routing and compatibility settings.
    pub providers: ProvidersConfig,
    /// Memory subsystem settings.
    pub memory: MemoryConfig,
    /// Learning governance settings.
    pub learning: LearningConfig,
    /// Plugin loading settings.
    pub plugins: PluginsConfig,
    /// Role loading settings.
    pub roles: RolesConfig,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            instance_name: "ai-office-swarm".into(),
            orchestrator: OrchestratorConfig::default(),
            telemetry: TelemetryConfig::default(),
            providers: ProvidersConfig::default(),
            memory: MemoryConfig::default(),
            learning: LearningConfig::default(),
            plugins: PluginsConfig::default(),
            roles: RolesConfig::default(),
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

/// Provider subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProvidersConfig {
    /// Whether provider-based model routing is enabled.
    pub enabled: bool,
    /// Whether routing should exclude unhealthy providers.
    pub require_healthy: bool,
    /// Preferred provider ID or short name.
    pub default_provider: Option<String>,
    /// Preferred model identifier.
    pub default_model: Option<String>,
    /// Explicit provider allowlist (empty = any configured provider).
    pub allowlist: Vec<String>,
    /// Explicit provider deny list.
    pub blocklist: Vec<String>,
    /// Routing behavior for provider selection.
    pub routing: ProviderRoutingConfig,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_healthy: true,
            default_provider: None,
            default_model: None,
            allowlist: Vec::new(),
            blocklist: Vec::new(),
            routing: ProviderRoutingConfig::default(),
        }
    }
}

/// Strategy-level configuration for provider routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderRoutingConfig {
    /// Primary routing strategy to use.
    pub strategy: ProviderRoutingStrategy,
    /// Whether fallback providers may be used automatically.
    pub fallback_allowed: bool,
    /// Cost preference used by cost-aware strategies.
    pub cost_preference: RoutingCostPreference,
    /// Latency preference used by latency-aware strategies.
    pub latency_preference: RoutingLatencyPreference,
}

impl Default for ProviderRoutingConfig {
    fn default() -> Self {
        Self {
            strategy: ProviderRoutingStrategy::default(),
            fallback_allowed: true,
            cost_preference: RoutingCostPreference::default(),
            latency_preference: RoutingLatencyPreference::default(),
        }
    }
}

/// Supported provider routing strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRoutingStrategy {
    /// Select the first provider that satisfies capability requirements.
    #[default]
    CapabilityMatch,
    /// Select the provider with the lowest configured cost score.
    LowestCost,
    /// Select the provider with the lowest configured latency score.
    LowestLatency,
    /// Distribute requests across matching providers in round-robin order.
    RoundRobin,
}

/// Cost preference used by the provider router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingCostPreference {
    /// Prefer the cheapest provider.
    Cheapest,
    /// Balance quality and cost.
    #[default]
    Balanced,
    /// Prefer quality even at higher cost.
    BestQuality,
}

/// Latency preference used by the provider router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingLatencyPreference {
    /// Prefer the fastest provider.
    Fastest,
    /// Balance latency and quality.
    #[default]
    Balanced,
    /// Do not optimize for latency.
    NoPreference,
}

/// Memory subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Backend implementation to use.
    pub backend: MemoryBackendKind,
    /// Whether retention policies should be enforced automatically.
    pub auto_apply_retention: bool,
    /// Whether known sensitive fields should be redacted before persistence.
    pub redact_personal_data: bool,
    /// Interval for retention sweeps in seconds.
    pub retention_interval_secs: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: MemoryBackendKind::default(),
            auto_apply_retention: true,
            redact_personal_data: true,
            retention_interval_secs: 300,
        }
    }
}

/// Supported memory backend kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryBackendKind {
    /// Built-in in-memory backend.
    #[default]
    InMemory,
    /// Plugin-provided backend implementation.
    Plugin,
}

/// Learning subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    /// Whether learning is enabled at all.
    pub enabled: bool,
    /// Whether new outputs require approval unless a narrower policy overrides it.
    pub require_approval_by_default: bool,
    /// Maximum number of pending outputs before learning should pause.
    pub max_pending_outputs: u64,
    /// Default scope used for operator-facing governance and approval queues.
    pub default_scope: LearningScopeKind,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            require_approval_by_default: true,
            max_pending_outputs: 100,
            default_scope: LearningScopeKind::default(),
        }
    }
}

/// Canonical learning scope labels for configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LearningScopeKind {
    /// Agent-local learning.
    Agent,
    /// Team-level learning.
    Team,
    /// Tenant-level learning.
    #[default]
    Tenant,
    /// Workflow-specific learning.
    Workflow,
    /// Global learning.
    Global,
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

/// Role subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RolesConfig {
    /// Directory containing role definition files.
    pub roles_dir: Option<String>,
    /// Whether to auto-load roles on startup.
    pub auto_load: bool,
    /// Whether to reject roles with validation errors (true) or log warnings and continue (false).
    pub strict_validation: bool,
}

impl Default for RolesConfig {
    fn default() -> Self {
        Self {
            roles_dir: Some("roles".into()),
            auto_load: true,
            strict_validation: false,
        }
    }
}
