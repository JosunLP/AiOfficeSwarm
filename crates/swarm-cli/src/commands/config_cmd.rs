//! `swarm config` sub-command: prints the effective configuration.

use clap::Args;
use swarm_config::SwarmConfig;

/// Config command arguments.
#[derive(Args)]
pub struct ConfigArgs {
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

pub async fn run(args: ConfigArgs, config: &SwarmConfig) -> anyhow::Result<()> {
    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(config)?;
            println!("{}", json);
        }
        _ => {
            println!("Effective configuration:");
            println!("  instance_name:                {}", config.instance_name);
            println!("  orchestrator:");
            println!(
                "    event_channel_capacity:     {}",
                config.orchestrator.event_channel_capacity
            );
            println!(
                "    max_dispatch_per_tick:      {}",
                config.orchestrator.max_dispatch_per_tick
            );
            println!(
                "    default_task_timeout_secs:  {}",
                config.orchestrator.default_task_timeout_secs
            );
            println!(
                "    max_concurrent_tasks:       {}",
                config.orchestrator.max_concurrent_tasks
            );
            println!(
                "    task_store_path:            {}",
                config.orchestrator.task_store_path
            );
            println!("  telemetry:");
            println!(
                "    log_level:                  {}",
                config.telemetry.log_level
            );
            println!(
                "    log_format:                 {:?}",
                config.telemetry.log_format
            );
            println!(
                "    otlp_enabled:               {}",
                config.telemetry.otlp_enabled
            );
            println!("  providers:");
            println!(
                "    enabled:                    {}",
                config.providers.enabled
            );
            println!(
                "    require_healthy:            {}",
                config.providers.require_healthy
            );
            println!(
                "    default_provider:           {}",
                config
                    .providers
                    .default_provider
                    .as_deref()
                    .unwrap_or("<none>")
            );
            println!(
                "    default_model:              {}",
                config
                    .providers
                    .default_model
                    .as_deref()
                    .unwrap_or("<none>")
            );
            println!(
                "    routing.strategy:           {:?}",
                config.providers.routing.strategy
            );
            println!(
                "    routing.fallback_allowed:   {}",
                config.providers.routing.fallback_allowed
            );
            println!("  memory:");
            println!(
                "    backend:                    {:?}",
                config.memory.backend
            );
            println!(
                "    auto_apply_retention:       {}",
                config.memory.auto_apply_retention
            );
            println!(
                "    redact_personal_data:       {}",
                config.memory.redact_personal_data
            );
            println!(
                "    retention_interval_secs:    {}",
                config.memory.retention_interval_secs
            );
            println!("  learning:");
            println!(
                "    enabled:                    {}",
                config.learning.enabled
            );
            println!(
                "    require_approval_by_default:{}",
                config.learning.require_approval_by_default
            );
            println!(
                "    max_pending_outputs:        {}",
                config.learning.max_pending_outputs
            );
            println!(
                "    default_scope:              {:?}",
                config.learning.default_scope
            );
            println!(
                "    store_path:                 {}",
                config.learning.store_path
            );
            println!("  roles:");
            println!(
                "    roles_dir:                  {}",
                config.roles.roles_dir.as_deref().unwrap_or("<none>")
            );
            println!("    auto_load:                  {}", config.roles.auto_load);
            println!(
                "    strict_validation:          {}",
                config.roles.strict_validation
            );
        }
    }
    Ok(())
}
