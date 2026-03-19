//! # swarm CLI
//!
//! The command-line management interface for the AiOfficeSwarm framework.
//!
//! ## Usage
//! ```text
//! swarm [OPTIONS] <COMMAND>
//!
//! Commands:
//!   agent    Manage agents (list, register, deregister, status)
//!   task     Manage tasks (submit, list, cancel, status)
//!   plugin   Manage plugins (list, load, unload, invoke)
//!   role     Inspect and validate organizational role definitions
//!   learning Inspect learning governance defaults
//!   config   Show effective configuration
//!   metrics  Show runtime metrics
//!   update   Download and install the latest released CLI binary
//!   demo     Run a built-in demonstration swarm
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

use clap::{Parser, Subcommand};

mod commands;

use commands::{agent, config_cmd, demo, learning, metrics_cmd, plugin, role, task, update};

/// AiOfficeSwarm — enterprise AI agent orchestration framework.
#[derive(Parser)]
#[command(
    name = "swarm",
    version = env!("CARGO_PKG_VERSION"),
    author,
    about = "Enterprise AI agent orchestration framework",
    long_about = None,
)]
struct Cli {
    /// Path to the configuration file (TOML).
    #[arg(short, long, env = "SWARM_CONFIG", global = true)]
    config: Option<String>,

    /// Override the log level (e.g., debug, info, warn, error).
    #[arg(short, long, env = "SWARM_LOG_LEVEL", global = true)]
    log_level: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

/// Top-level CLI sub-commands.
#[derive(Subcommand)]
enum Commands {
    /// Agent management commands.
    Agent(agent::AgentArgs),
    /// Task management commands.
    Task(task::TaskArgs),
    /// Plugin management commands.
    Plugin(plugin::PluginArgs),
    /// Role inspection and validation commands.
    Role(role::RoleArgs),
    /// Learning governance inspection commands.
    Learning(learning::LearningArgs),
    /// Show effective configuration.
    Config(config_cmd::ConfigArgs),
    /// Show runtime metrics.
    Metrics(metrics_cmd::MetricsArgs),
    /// Download and install the latest released CLI binary.
    Update(update::UpdateArgs),
    /// Run the built-in demonstration swarm.
    Demo(demo::DemoArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load configuration.
    let mut config = match &cli.config {
        Some(path) => swarm_config::ConfigLoader::from_file(path).unwrap_or_else(|e| {
            eprintln!("Warning: could not load config file '{}': {}", path, e);
            swarm_config::ConfigLoader::defaults()
        }),
        None => swarm_config::ConfigLoader::defaults(),
    };

    // Apply CLI log-level override.
    if let Some(level) = &cli.log_level {
        config.telemetry.log_level = level.clone();
    }

    config = swarm_config::ConfigLoader::with_env_overrides(config);

    // Initialize telemetry.
    swarm_telemetry::init_tracing(&config.telemetry);

    // Dispatch to sub-command.
    match cli.command {
        Commands::Agent(args) => agent::run(args, &config).await,
        Commands::Task(args) => task::run(args, &config).await,
        Commands::Plugin(args) => plugin::run(args, &config).await,
        Commands::Role(args) => role::run(args, &config).await,
        Commands::Learning(args) => learning::run(args, &config).await,
        Commands::Config(args) => config_cmd::run(args, &config).await,
        Commands::Metrics(args) => metrics_cmd::run(args, &config).await,
        Commands::Update(args) => update::run(args, &config).await,
        Commands::Demo(args) => demo::run(args, &config).await,
    }
}
