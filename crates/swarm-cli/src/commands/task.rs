//! `swarm task` sub-commands.

use clap::{Args, Subcommand};
use swarm_config::SwarmConfig;

/// Task management arguments.
#[derive(Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub subcommand: TaskSubcommand,
}

#[derive(Subcommand)]
pub enum TaskSubcommand {
    /// List all tasks.
    List,
    /// Submit a new task with a JSON payload.
    Submit {
        /// Task name.
        #[arg(short, long)]
        name: String,
        /// JSON input payload.
        #[arg(short, long, default_value = "{}")]
        input: String,
    },
}

pub async fn run(args: TaskArgs, _config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        TaskSubcommand::List => {
            println!("Tasks: (no persistent store in this session — use the demo command)");
        }
        TaskSubcommand::Submit { name, input } => {
            let payload: serde_json::Value = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("Invalid JSON input: {}", e))?;
            println!(
                "Task '{}' would be submitted with payload: {}",
                name, payload
            );
            println!("(connect to a running swarm instance for actual submission)");
        }
    }
    Ok(())
}
