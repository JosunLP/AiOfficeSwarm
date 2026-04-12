//! `swarm task` sub-commands.

use clap::{Args, Subcommand};
use std::{str::FromStr, time::Duration};
use swarm_config::SwarmConfig;
use swarm_core::{
    capability::Capability,
    identity::TaskId,
    task::{Task, TaskPriority, TaskSpec},
};
use swarm_orchestrator::{FileTaskStore, TaskStore};

/// Task management arguments.
#[derive(Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub subcommand: TaskSubcommand,
}

#[derive(Subcommand)]
pub enum TaskSubcommand {
    /// List locally persisted tasks.
    List(TaskListArgs),
    /// Submit a new task into the local persistent task store.
    Submit(TaskSubmitArgs),
    /// Show a single persisted task.
    Status(TaskStatusArgs),
    /// Cancel a pending persisted task.
    Cancel(TaskCancelArgs),
}

/// Arguments for listing persisted tasks.
#[derive(Args)]
pub struct TaskListArgs {
    /// Filter by lifecycle status.
    #[arg(long)]
    pub status: Option<String>,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for creating a persisted task.
#[derive(Args)]
pub struct TaskSubmitArgs {
    /// Task name.
    #[arg(short, long)]
    pub name: String,
    /// JSON input payload.
    #[arg(short, long, default_value = "{}")]
    pub input: String,
    /// Priority: low, normal, high, critical.
    #[arg(long, default_value = "normal")]
    pub priority: String,
    /// Required capability. Repeat to require multiple capabilities.
    #[arg(long = "capability")]
    pub capabilities: Vec<String>,
    /// Metadata entry in key=value form. Repeat for multiple entries.
    #[arg(long = "metadata")]
    pub metadata: Vec<String>,
    /// Timeout in seconds. Use 0 to disable the timeout.
    #[arg(long)]
    pub timeout_secs: Option<u64>,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for showing a single task.
#[derive(Args)]
pub struct TaskStatusArgs {
    /// Task ID.
    pub id: String,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for cancelling a task.
#[derive(Args)]
pub struct TaskCancelArgs {
    /// Task ID.
    pub id: String,
    /// Optional cancellation reason.
    #[arg(long)]
    pub reason: Option<String>,
}

fn task_store(config: &SwarmConfig) -> FileTaskStore {
    FileTaskStore::new(&config.orchestrator.task_store_path)
}

fn parse_task_id(id: &str) -> anyhow::Result<TaskId> {
    TaskId::from_str(id).map_err(|error| anyhow::anyhow!("invalid task id '{id}': {error}"))
}

fn parse_priority(value: &str) -> anyhow::Result<TaskPriority> {
    match value.trim().to_ascii_lowercase().as_str() {
        "low" => Ok(TaskPriority::Low),
        "normal" => Ok(TaskPriority::Normal),
        "high" => Ok(TaskPriority::High),
        "critical" => Ok(TaskPriority::Critical),
        other => anyhow::bail!(
            "unsupported task priority '{other}' (expected low, normal, high, or critical)"
        ),
    }
}

fn parse_metadata(entries: &[String]) -> anyhow::Result<swarm_core::types::Metadata> {
    let mut metadata = swarm_core::types::Metadata::new();
    for entry in entries {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("invalid metadata entry '{entry}' (expected key=value)")
        })?;
        if key.trim().is_empty() {
            anyhow::bail!("invalid metadata entry '{entry}' (key must not be empty)");
        }
        metadata.insert(key.trim(), value.trim());
    }
    Ok(metadata)
}

fn build_task_spec(args: &TaskSubmitArgs, config: &SwarmConfig) -> anyhow::Result<TaskSpec> {
    let payload: serde_json::Value = serde_json::from_str(&args.input)
        .map_err(|e| anyhow::anyhow!("invalid JSON input: {}", e))?;
    let mut spec = TaskSpec::new(&args.name, payload);
    spec.priority = parse_priority(&args.priority)?;
    spec.metadata = parse_metadata(&args.metadata)?;
    spec.timeout = match args.timeout_secs {
        Some(0) => None,
        Some(seconds) => Some(Duration::from_secs(seconds)),
        None if config.orchestrator.default_task_timeout_secs == 0 => None,
        None => Some(Duration::from_secs(
            config.orchestrator.default_task_timeout_secs,
        )),
    };
    for capability in &args.capabilities {
        if capability.trim().is_empty() {
            anyhow::bail!("capability names must not be empty");
        }
        spec.required_capabilities
            .add(Capability::new(capability.trim()));
    }
    spec.validate()?;
    Ok(spec)
}

fn status_matches(task: &Task, status: Option<&str>) -> bool {
    status
        .map(|value| value.trim().eq_ignore_ascii_case(task.status.label()))
        .unwrap_or(true)
}

fn print_task_text(task: &Task) -> anyhow::Result<()> {
    println!("Task");
    println!("  id:           {}", task.id);
    println!("  name:         {}", task.spec.name);
    println!("  status:       {}", task.status.label());
    println!("  priority:     {:?}", task.spec.priority);
    println!("  created_at:   {}", task.created_at.to_rfc3339());
    println!("  updated_at:   {}", task.updated_at.to_rfc3339());
    println!(
        "  timeout_secs: {}",
        task.spec
            .timeout
            .map(|value| value.as_secs().to_string())
            .unwrap_or_else(|| "<none>".into())
    );
    println!(
        "  capabilities: {}",
        if task.spec.required_capabilities.is_empty() {
            "<none>".into()
        } else {
            task.spec
                .required_capabilities
                .iter()
                .map(|capability| capability.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "  metadata:     {}",
        if task.spec.metadata.is_empty() {
            "<none>".into()
        } else {
            serde_json::to_string_pretty(&task.spec.metadata)?
        }
    );
    println!(
        "  input:        {}",
        serde_json::to_string_pretty(&task.spec.input)?
    );
    Ok(())
}

pub async fn run(args: TaskArgs, config: &SwarmConfig) -> anyhow::Result<()> {
    match args.subcommand {
        TaskSubcommand::List(args) => {
            let mut tasks = task_store(config).list().await?;
            if let Some(status) = args.status.as_deref() {
                tasks.retain(|task| status_matches(task, Some(status)));
            }

            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&tasks)?),
                _ => {
                    println!("Persisted tasks ({})", tasks.len());
                    for task in tasks {
                        println!(
                            "  - {} [{}] {}",
                            task.id,
                            task.status.label(),
                            task.spec.name
                        );
                    }
                }
            }
        }
        TaskSubcommand::Submit(args) => {
            let task = Task::new(build_task_spec(&args, config)?);
            task_store(config).record(task.clone()).await?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&task)?),
                _ => {
                    println!("Submitted task {}", task.id);
                    println!("  name:     {}", task.spec.name);
                    println!("  status:   {}", task.status.label());
                    println!("  store:    {}", config.orchestrator.task_store_path);
                }
            }
        }
        TaskSubcommand::Status(args) => {
            let id = parse_task_id(&args.id)?;
            let task = task_store(config)
                .get(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&task)?),
                _ => print_task_text(&task)?,
            }
        }
        TaskSubcommand::Cancel(args) => {
            let id = parse_task_id(&args.id)?;
            let task = task_store(config).cancel(&id, args.reason).await?;
            println!(
                "Cancelled task {} (status={})",
                task.id,
                task.status.label()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::task::TaskStatus;
    use tempfile::tempdir;

    async fn test_config() -> (tempfile::TempDir, SwarmConfig) {
        let dir = tempdir().unwrap();
        let mut config = SwarmConfig::default();
        config.orchestrator.task_store_path =
            dir.path().join("task-store.json").display().to_string();
        (dir, config)
    }

    #[tokio::test]
    async fn submit_persists_task_snapshot() {
        let (_dir, config) = test_config().await;
        run(
            TaskArgs {
                subcommand: TaskSubcommand::Submit(TaskSubmitArgs {
                    name: "draft-plan".into(),
                    input: r#"{"goal":"expand"}"#.into(),
                    priority: "high".into(),
                    capabilities: vec!["planning".into()],
                    metadata: vec!["source=cli".into()],
                    timeout_secs: Some(60),
                    format: "json".into(),
                }),
            },
            &config,
        )
        .await
        .unwrap();

        let tasks = task_store(&config).list().await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].spec.name, "draft-plan");
        assert_eq!(tasks[0].status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn cancel_updates_pending_task_snapshot() {
        let (_dir, config) = test_config().await;
        let task = Task::new(TaskSpec::new("cancel-me", serde_json::json!({})));
        let id = task.id;
        task_store(&config).record(task).await.unwrap();

        run(
            TaskArgs {
                subcommand: TaskSubcommand::Cancel(TaskCancelArgs {
                    id: id.to_string(),
                    reason: Some("operator-request".into()),
                }),
            },
            &config,
        )
        .await
        .unwrap();

        let updated = task_store(&config).get(&id).await.unwrap().unwrap();
        assert!(matches!(updated.status, TaskStatus::Cancelled { .. }));
    }

    #[test]
    fn parse_priority_accepts_supported_labels() {
        assert_eq!(parse_priority("low").unwrap(), TaskPriority::Low);
        assert_eq!(parse_priority("normal").unwrap(), TaskPriority::Normal);
        assert_eq!(parse_priority("high").unwrap(), TaskPriority::High);
        assert_eq!(parse_priority("critical").unwrap(), TaskPriority::Critical);
    }
}
