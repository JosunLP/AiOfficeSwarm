//! `swarm metrics` sub-command: prints runtime metrics snapshot.

use clap::Args;
use swarm_config::SwarmConfig;
use swarm_telemetry::Metrics;

/// Metrics command arguments.
#[derive(Args)]
pub struct MetricsArgs {
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

pub async fn run(args: MetricsArgs, _config: &SwarmConfig) -> anyhow::Result<()> {
    // In a real deployment, metrics would be fetched from a running process.
    // Here we show a zeroed snapshot for demonstration.
    let m = Metrics::new();
    let snap = m.snapshot();

    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&snap)?;
            println!("{}", json);
        }
        _ => {
            println!("Runtime metrics snapshot:");
            println!("  tasks_submitted:    {}", snap.tasks_submitted);
            println!("  tasks_completed:    {}", snap.tasks_completed);
            println!("  tasks_failed:       {}", snap.tasks_failed);
            println!("  tasks_cancelled:    {}", snap.tasks_cancelled);
            println!("  agents_registered:  {}", snap.agents_registered);
            println!("  policy_evaluations: {}", snap.policy_evaluations);
            println!("  policy_denials:     {}", snap.policy_denials);
            println!("  plugin_invocations: {}", snap.plugin_invocations);
        }
    }
    Ok(())
}
