//! `swarm learning` sub-commands.

use clap::{Args, Subcommand};
use swarm_config::SwarmConfig;

/// Learning management arguments.
#[derive(Args)]
pub struct LearningArgs {
    #[command(subcommand)]
    pub subcommand: LearningSubcommand,
}

/// Learning management sub-commands.
#[derive(Subcommand)]
pub enum LearningSubcommand {
    /// Show the effective learning governance posture from configuration.
    Inspect,
}

pub async fn run(args: LearningArgs, config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        LearningSubcommand::Inspect => {
            println!("Learning governance");
            println!("  enabled: {}", config.learning.enabled);
            println!(
                "  require_approval_by_default: {}",
                config.learning.require_approval_by_default
            );
            println!(
                "  max_pending_outputs: {}",
                config.learning.max_pending_outputs
            );
            println!("  default_scope: {:?}", config.learning.default_scope);
            println!();
            println!(
                "Note: approval queue persistence is not yet wired into the CLI runtime; this command reports the configured governance baseline."
            );
        }
    }
    Ok(())
}
