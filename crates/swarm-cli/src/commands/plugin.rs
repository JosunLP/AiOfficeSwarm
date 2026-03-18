//! `swarm plugin` sub-commands.

use clap::{Args, Subcommand};
use swarm_config::SwarmConfig;

/// Plugin management arguments.
#[derive(Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub subcommand: PluginSubcommand,
}

#[derive(Subcommand)]
pub enum PluginSubcommand {
    /// List all loaded plugins.
    List,
}

pub async fn run(args: PluginArgs, _config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        PluginSubcommand::List => {
            println!(
                "Loaded plugins: (no persistent store in this session — use the demo command)"
            );
        }
    }
    Ok(())
}
