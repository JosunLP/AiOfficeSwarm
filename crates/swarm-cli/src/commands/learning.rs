//! `swarm learning` sub-commands.

use clap::{Args, Subcommand};
use std::str::FromStr;

use swarm_config::{model::LearningScopeKind, SwarmConfig};
use swarm_learning::{
    output::LearningRuleId, FileLearningStore, LearningOutput, LearningScope, LearningStore,
};

/// Learning management arguments.
#[derive(Args)]
pub struct LearningArgs {
    /// Learning subcommand to execute.
    #[command(subcommand)]
    pub subcommand: LearningSubcommand,
}

/// Learning management sub-commands.
#[derive(Subcommand)]
pub enum LearningSubcommand {
    /// Show the effective learning governance posture from configuration.
    Inspect,
    /// List pending approval items from the persistent learning queue.
    Pending(LearningPendingArgs),
    /// Show a single learning output by ID.
    Get(LearningItemArgs),
    /// Approve a learning output.
    Approve(LearningActionArgs),
    /// Reject a learning output.
    Reject(LearningActionArgs),
    /// Roll back a previously applied learning output.
    Rollback(LearningActionArgs),
}

/// Arguments for listing pending learning outputs.
#[derive(Args)]
pub struct LearningPendingArgs {
    /// Scope to inspect (agent, team, tenant, workflow, global).
    #[arg(long)]
    pub scope: Option<String>,
    /// Scope identifier for non-global scopes.
    #[arg(long)]
    pub scope_id: Option<String>,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for showing a learning output.
#[derive(Args)]
pub struct LearningItemArgs {
    /// Learning output ID.
    pub id: String,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for updating a learning output lifecycle state.
#[derive(Args)]
pub struct LearningActionArgs {
    /// Learning output ID.
    pub id: String,
}

fn learning_store(config: &SwarmConfig) -> FileLearningStore {
    FileLearningStore::new(&config.learning.store_path)
}

fn scope_kind_name(scope: LearningScopeKind) -> &'static str {
    match scope {
        LearningScopeKind::Agent => "agent",
        LearningScopeKind::Team => "team",
        LearningScopeKind::Tenant => "tenant",
        LearningScopeKind::Workflow => "workflow",
        LearningScopeKind::Global => "global",
    }
}

fn resolve_scope(
    requested_scope: Option<&str>,
    scope_id: Option<&str>,
    config: &SwarmConfig,
) -> anyhow::Result<LearningScope> {
    let scope_name =
        requested_scope.unwrap_or_else(|| scope_kind_name(config.learning.default_scope));
    match scope_name.trim().to_ascii_lowercase().as_str() {
        "agent" => Ok(LearningScope::Agent {
            agent_id: required_scope_id("agent", scope_id)?.into(),
        }),
        "team" => Ok(LearningScope::Team {
            team_id: required_scope_id("team", scope_id)?.into(),
        }),
        "tenant" => Ok(LearningScope::Tenant {
            tenant_id: required_scope_id("tenant", scope_id)?.into(),
        }),
        "workflow" => Ok(LearningScope::Workflow {
            workflow_id: required_scope_id("workflow", scope_id)?.into(),
        }),
        "global" => Ok(LearningScope::Global),
        other => anyhow::bail!(
            "unsupported learning scope '{other}' (expected agent, team, tenant, workflow, or global)"
        ),
    }
}

fn required_scope_id<'a>(scope: &str, scope_id: Option<&'a str>) -> anyhow::Result<&'a str> {
    scope_id.ok_or_else(|| anyhow::anyhow!("--scope-id is required when --scope {scope} is used"))
}

fn parse_learning_rule_id(id: &str) -> anyhow::Result<LearningRuleId> {
    LearningRuleId::from_str(id)
        .map_err(|error| anyhow::anyhow!("invalid learning output id '{id}': {error}"))
}

fn learning_scope_label(output: &LearningOutput) -> String {
    if let Some(agent_id) = output.agent_id.as_deref() {
        format!("agent:{agent_id}")
    } else if let Some(tenant_id) = output.tenant_id.as_deref() {
        format!("tenant:{tenant_id}")
    } else {
        "global".into()
    }
}

fn print_learning_output_text(output: &LearningOutput) -> anyhow::Result<()> {
    println!("Learning output");
    println!("  id:                {}", output.id);
    println!("  category:          {}", output.category.label());
    println!("  status:            {}", output.status.label());
    println!("  requires_approval: {}", output.requires_approval);
    println!("  scope:             {}", learning_scope_label(output));
    println!("  created_at:        {}", output.created_at.to_rfc3339());
    println!(
        "  applied_at:        {}",
        output
            .applied_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "<not applied>".into())
    );
    println!("  description:       {}", output.description);
    println!(
        "  delta:             {}",
        serde_json::to_string_pretty(&output.delta)?
    );
    println!(
        "  context:           {}",
        serde_json::to_string_pretty(&output.context)?
    );
    Ok(())
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
            println!("  store_path: {}", config.learning.store_path);
            println!();
            println!("Use `swarm learning pending`, `get`, `approve`, `reject`, and `rollback` to inspect and manage the persistent queue.");
        }
        LearningSubcommand::Pending(args) => {
            let scope = resolve_scope(args.scope.as_deref(), args.scope_id.as_deref(), config)?;
            let pending = learning_store(config)
                .list_pending_approvals(&scope)
                .await?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&pending)?),
                _ => {
                    println!(
                        "Pending learning outputs ({}, scope={})",
                        pending.len(),
                        scope.label()
                    );
                    for output in pending {
                        println!(
                            "  - {} [{}] {} {}",
                            output.id,
                            output.status.label(),
                            output.category.label(),
                            output.description
                        );
                    }
                }
            }
        }
        LearningSubcommand::Get(args) => {
            let id = parse_learning_rule_id(&args.id)?;
            let output = learning_store(config)
                .get(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("learning output {id} not found"))?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&output)?),
                _ => print_learning_output_text(&output)?,
            }
        }
        LearningSubcommand::Approve(args) => {
            let id = parse_learning_rule_id(&args.id)?;
            let store = learning_store(config);
            store.approve(&id).await?;
            let output = store
                .get(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("learning output {id} not found after approval"))?;
            println!(
                "Approved learning output {} (status={})",
                output.id,
                output.status.label()
            );
        }
        LearningSubcommand::Reject(args) => {
            let id = parse_learning_rule_id(&args.id)?;
            let store = learning_store(config);
            store.reject(&id).await?;
            let output = store
                .get(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("learning output {id} not found after rejection"))?;
            println!(
                "Rejected learning output {} (status={})",
                output.id,
                output.status.label()
            );
        }
        LearningSubcommand::Rollback(args) => {
            let id = parse_learning_rule_id(&args.id)?;
            let store = learning_store(config);
            store.rollback(&id).await?;
            let output = store
                .get(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("learning output {id} not found after rollback"))?;
            println!(
                "Rolled back learning output {} (status={})",
                output.id,
                output.status.label()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scope_uses_global_without_id() {
        let mut config = SwarmConfig::default();
        config.learning.default_scope = LearningScopeKind::Global;

        let scope = resolve_scope(None, None, &config).unwrap();
        assert_eq!(scope, LearningScope::Global);
    }

    #[test]
    fn resolve_scope_requires_scope_id_for_tenant_scope() {
        let config = SwarmConfig::default();
        let error = resolve_scope(Some("tenant"), None, &config).unwrap_err();
        assert!(error.to_string().contains("--scope-id is required"));
    }

    #[test]
    fn learning_scope_label_prefers_agent_scope() {
        let mut output = LearningOutput::auto(
            swarm_learning::output::LearningCategory::PreferenceAdaptation,
            "Test",
            serde_json::json!({}),
        );
        output.agent_id = Some("agent-1".into());
        output.tenant_id = Some("tenant-1".into());

        assert_eq!(learning_scope_label(&output), "agent:agent-1");
    }
}
