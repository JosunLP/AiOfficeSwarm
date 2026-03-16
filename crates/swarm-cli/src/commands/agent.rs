//! `swarm agent` sub-commands.

use clap::{Args, Subcommand};
use swarm_config::SwarmConfig;

/// Agent management arguments.
#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub subcommand: AgentSubcommand,
}

#[derive(Subcommand)]
pub enum AgentSubcommand {
    /// List all registered agents.
    List,
}

pub async fn run(args: AgentArgs, _config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        AgentSubcommand::List => {
            println!("Registered agents:");
            println!("  (no persistent store in this session — use the demo command to see agents in action)");
        }
    }
    Ok(())
}
