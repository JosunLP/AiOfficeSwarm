//! `swarm role` sub-commands.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Subcommand};
use swarm_config::SwarmConfig;
use swarm_role::{loader::LoadSummary, RoleLoader, RoleRegistry, ValidationSeverity};

/// Role management arguments.
#[derive(Args)]
pub struct RoleArgs {
    #[command(subcommand)]
    pub subcommand: RoleSubcommand,
}

/// Role management sub-commands.
#[derive(Subcommand)]
pub enum RoleSubcommand {
    /// List all roles that can be loaded from disk.
    List {
        /// Override the configured roles directory.
        #[arg(long)]
        dir: Option<String>,
    },
    /// Validate role definitions and print issues.
    Validate {
        /// Override the configured roles directory.
        #[arg(long)]
        dir: Option<String>,
        /// Fail the command when warnings are present in addition to hard errors.
        #[arg(long, default_value_t = false)]
        strict: bool,
    },
}

pub async fn run(args: RoleArgs, config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        RoleSubcommand::List { dir } => list_roles(dir, config),
        RoleSubcommand::Validate { dir, strict } => validate_roles(dir, strict, config),
    }
}

fn resolve_roles_dir(config: &SwarmConfig, dir: Option<String>) -> PathBuf {
    dir.map(PathBuf::from)
        .or_else(|| config.roles.roles_dir.as_ref().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("roles"))
}

fn load_summary(dir: &Path) -> anyhow::Result<LoadSummary> {
    let registry = RoleRegistry::new();
    RoleLoader::load_directory(dir, &registry)
        .with_context(|| format!("failed to load roles from '{}'", dir.display()))
}

fn list_roles(dir: Option<String>, config: &SwarmConfig) -> anyhow::Result<()> {
    let roles_dir = resolve_roles_dir(config, dir);
    let summary = load_summary(&roles_dir)?;

    println!("Role directory: {}", roles_dir.display());
    println!(
        "Loaded {} of {} role files ({} warnings, {} errors)",
        summary.loaded, summary.total_files, summary.warnings, summary.errors
    );

    for result in summary
        .results
        .iter()
        .filter(|result| result.spec.is_some())
    {
        let spec = result.spec.as_ref().expect("checked above");
        println!(
            "- {} | department={:?} | kind={:?} | path={}",
            spec.name, spec.department, spec.agent_kind, result.path
        );
    }

    Ok(())
}

fn validate_roles(dir: Option<String>, strict: bool, config: &SwarmConfig) -> anyhow::Result<()> {
    let roles_dir = resolve_roles_dir(config, dir);
    let summary = load_summary(&roles_dir)?;

    println!("Validating roles in {}", roles_dir.display());
    println!(
        "Summary: {} loaded / {} files, {} warnings, {} errors",
        summary.loaded, summary.total_files, summary.warnings, summary.errors
    );

    for result in &summary.results {
        if result.error.is_none() && result.issues.is_empty() {
            continue;
        }

        println!("\n{}", result.path);
        if let Some(error) = &result.error {
            println!("  [error] {}", error);
        }
        for issue in &result.issues {
            let severity = match issue.severity {
                ValidationSeverity::Info => "info",
                ValidationSeverity::Warning => "warning",
                ValidationSeverity::Error => "error",
            };
            println!("  [{}] {}: {}", severity, issue.field, issue.message);
        }
    }

    let has_warnings = summary.warnings > 0;
    let has_errors = summary.errors > 0
        || summary.results.iter().any(|result| {
            result
                .issues
                .iter()
                .any(|issue| issue.severity == ValidationSeverity::Error)
        });

    if has_errors || (strict && has_warnings) {
        anyhow::bail!(
            "role validation failed (strict={}, warnings={}, errors={})",
            strict,
            summary.warnings,
            summary.errors
        );
    }

    println!("\nRole validation passed.");
    Ok(())
}
