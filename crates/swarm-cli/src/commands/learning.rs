//! `swarm learning` sub-commands.

use clap::{Args, Subcommand};
use std::str::FromStr;

use swarm_config::{model::LearningScopeKind, SwarmConfig};
use swarm_learning::{
    output::LearningRuleId, FileLearningStore, LearningCategory, LearningLifecycleAction,
    LearningOutput, LearningOutputFilter, LearningScope, LearningStatus, LearningStore,
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
    /// List recorded learning outputs for a scope.
    List(LearningListArgs),
    /// List pending approval items from the persistent learning queue.
    Pending(LearningPendingArgs),
    /// Show a single learning output by ID.
    Get(LearningItemArgs),
    /// Approve a learning output.
    Approve(LearningActionArgs),
    /// Approve multiple learning outputs that match the given filters.
    ApproveBatch(LearningBatchActionArgs),
    /// Reject a learning output.
    Reject(LearningActionArgs),
    /// Reject multiple learning outputs that match the given filters.
    RejectBatch(LearningBatchActionArgs),
    /// Roll back a previously applied learning output.
    Rollback(LearningActionArgs),
    /// Roll back multiple learning outputs that match the given filters.
    RollbackBatch(LearningBatchActionArgs),
}

/// Arguments for listing learning outputs.
#[derive(Args)]
pub struct LearningListArgs {
    /// Scope to inspect (agent, team, tenant, workflow, global).
    #[arg(long)]
    pub scope: Option<String>,
    /// Scope identifier for non-global scopes.
    #[arg(long)]
    pub scope_id: Option<String>,
    /// Optional category filter (for example `plan_template`).
    #[arg(long)]
    pub category: Option<String>,
    /// Optional status filter (pending, pending_approval, applied, rejected, rolled_back).
    #[arg(long)]
    pub status: Option<String>,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
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
    /// Optional category filter (for example `plan_template`).
    #[arg(long)]
    pub category: Option<String>,
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

/// Arguments for batch lifecycle updates.
#[derive(Args)]
pub struct LearningBatchActionArgs {
    /// Scope to inspect (agent, team, tenant, workflow, global).
    #[arg(long)]
    pub scope: Option<String>,
    /// Scope identifier for non-global scopes.
    #[arg(long)]
    pub scope_id: Option<String>,
    /// Optional category filter (for example `plan_template`).
    #[arg(long)]
    pub category: Option<String>,
    /// Optional status filter (pending, pending_approval, applied, rejected, rolled_back).
    #[arg(long)]
    pub status: Option<String>,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
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
    output.scope_label()
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

fn print_learning_output_summary(output: &LearningOutput) {
    println!(
        "  - {} [{}] {} {} ({})",
        output.id,
        output.status.label(),
        output.category.label(),
        output.description,
        output.scope_label()
    );
}

fn parse_learning_category(label: &str) -> anyhow::Result<LearningCategory> {
    LearningCategory::from_str(label).map_err(|error| {
        anyhow::anyhow!(
            "unsupported learning category '{label}': {error} (expected a built-in label such as plan_template or a custom snake_case value)"
        )
    })
}

fn parse_learning_status(label: &str) -> anyhow::Result<LearningStatus> {
    match label.trim().to_ascii_lowercase().as_str() {
        "pending" => Ok(LearningStatus::Pending),
        "pending_approval" => Ok(LearningStatus::PendingApproval),
        "applied" => Ok(LearningStatus::Applied),
        "rejected" => Ok(LearningStatus::Rejected),
        "rolled_back" => Ok(LearningStatus::RolledBack),
        other => anyhow::bail!(
            "unsupported learning status '{other}' (expected pending, pending_approval, applied, rejected, or rolled_back)"
        ),
    }
}

fn learning_output_filter(
    category: Option<&str>,
    status: Option<&str>,
) -> anyhow::Result<LearningOutputFilter> {
    Ok(LearningOutputFilter {
        category: category.map(parse_learning_category).transpose()?,
        status: status.map(parse_learning_status).transpose()?,
    })
}

fn format_batch_action_verb(action: LearningLifecycleAction) -> &'static str {
    match action {
        LearningLifecycleAction::Approve => "Approved",
        LearningLifecycleAction::Reject => "Rejected",
        LearningLifecycleAction::Rollback => "Rolled back",
    }
}

fn default_status_for_action(action: LearningLifecycleAction) -> LearningStatus {
    match action {
        LearningLifecycleAction::Approve | LearningLifecycleAction::Reject => {
            LearningStatus::PendingApproval
        }
        LearningLifecycleAction::Rollback => LearningStatus::Applied,
    }
}

async fn run_batch_action(
    args: LearningBatchActionArgs,
    config: &SwarmConfig,
    action: LearningLifecycleAction,
) -> anyhow::Result<()> {
    let scope = resolve_scope(args.scope.as_deref(), args.scope_id.as_deref(), config)?;
    let mut filter = learning_output_filter(args.category.as_deref(), args.status.as_deref())?;
    if filter.status.is_none() {
        filter.status = Some(default_status_for_action(action));
    }

    let updated = learning_store(config)
        .update_matching(&scope, &filter, action)
        .await?;

    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&updated)?),
        _ => {
            println!(
                "{} learning outputs ({}, scope={}, action={})",
                format_batch_action_verb(action),
                updated.len(),
                scope.label(),
                action.label()
            );
            for output in updated {
                print_learning_output_summary(&output);
            }
        }
    }

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
            println!("Use `swarm learning list`, `pending`, `get`, `approve`, `approve-batch`, `reject`, `reject-batch`, `rollback`, and `rollback-batch` to inspect and manage the persistent queue.");
        }
        LearningSubcommand::List(args) => {
            let scope = resolve_scope(args.scope.as_deref(), args.scope_id.as_deref(), config)?;
            let filter = learning_output_filter(args.category.as_deref(), args.status.as_deref())?;
            let outputs = learning_store(config)
                .list_filtered(&scope, &filter)
                .await?;

            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&outputs)?),
                _ => {
                    println!(
                        "Learning outputs ({}, scope={})",
                        outputs.len(),
                        scope.label()
                    );
                    for output in outputs {
                        print_learning_output_summary(&output);
                    }
                }
            }
        }
        LearningSubcommand::Pending(args) => {
            let scope = resolve_scope(args.scope.as_deref(), args.scope_id.as_deref(), config)?;
            let pending = learning_store(config)
                .list_filtered(
                    &scope,
                    &LearningOutputFilter {
                        category: args
                            .category
                            .map(|value| parse_learning_category(&value))
                            .transpose()?,
                        status: Some(LearningStatus::PendingApproval),
                    },
                )
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
                        print_learning_output_summary(&output);
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
        LearningSubcommand::ApproveBatch(args) => {
            run_batch_action(args, config, LearningLifecycleAction::Approve).await?;
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
        LearningSubcommand::RejectBatch(args) => {
            run_batch_action(args, config, LearningLifecycleAction::Reject).await?;
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
        LearningSubcommand::RollbackBatch(args) => {
            run_batch_action(args, config, LearningLifecycleAction::Rollback).await?;
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
        output.set_scope(LearningScope::Agent {
            agent_id: "agent-1".into(),
        });

        assert_eq!(learning_scope_label(&output), "agent:agent-1");
    }

    #[test]
    fn learning_scope_label_supports_team_scope() {
        let mut output = LearningOutput::auto(
            swarm_learning::output::LearningCategory::PreferenceAdaptation,
            "Test",
            serde_json::json!({}),
        );
        output.set_scope(LearningScope::Team {
            team_id: "ops".into(),
        });

        assert_eq!(learning_scope_label(&output), "team:ops");
    }

    #[test]
    fn parse_learning_status_rejects_unknown_values() {
        let error = parse_learning_status("mystery").unwrap_err();
        assert!(error.to_string().contains("unsupported learning status"));
    }

    #[test]
    fn parse_learning_category_supports_builtin_labels() {
        let category = parse_learning_category("plan_template").unwrap();
        assert_eq!(category, LearningCategory::PlanTemplate);
    }

    #[test]
    fn learning_output_filter_parses_category_and_status() {
        let filter =
            learning_output_filter(Some("plan_template"), Some("pending_approval")).unwrap();
        assert_eq!(filter.category, Some(LearningCategory::PlanTemplate));
        assert_eq!(filter.status, Some(LearningStatus::PendingApproval));
    }

    #[test]
    fn learning_output_filter_matches_expected_output() {
        let filter =
            learning_output_filter(Some("plan_template"), Some("pending_approval")).unwrap();
        let output = LearningOutput::requires_review(
            LearningCategory::PlanTemplate,
            "Template",
            serde_json::json!({}),
            serde_json::json!({}),
        );

        assert!(filter.matches(&output));
    }
}
