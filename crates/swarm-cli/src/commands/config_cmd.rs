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
        }
    }
    Ok(())
}
