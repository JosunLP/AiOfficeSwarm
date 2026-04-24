//! `swarm task` sub-commands.

use async_trait::async_trait;
use clap::{Args, Subcommand};
use serde::Serialize;
use std::{str::FromStr, time::Duration};
use swarm_config::SwarmConfig;
use swarm_core::{
    agent::{Agent, AgentDescriptor, AgentKind},
    capability::{Capability, CapabilitySet},
    error::SwarmResult,
    identity::TaskId,
    task::{Task, TaskPriority, TaskSpec, TaskStatus},
};
use swarm_orchestrator::{FileTaskStore, Orchestrator, TaskStore};
use swarm_runtime::TaskRunner;

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
    /// Process pending persisted tasks with built-in local workers.
    Process(TaskProcessArgs),
    /// Return a retryable persisted task to the pending state.
    Retry(TaskRetryArgs),
    /// Return multiple retryable persisted tasks to the pending state.
    RetryBatch(TaskRetryBatchArgs),
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

/// Arguments for processing persisted tasks.
#[derive(Args)]
pub struct TaskProcessArgs {
    /// Maximum number of pending tasks to process in this invocation.
    #[arg(long)]
    pub limit: Option<usize>,
    /// Number of built-in local workers to register.
    #[arg(long, default_value_t = 1)]
    pub workers: usize,
    /// Output format: text or json.
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

/// Arguments for retrying a persisted task.
#[derive(Args)]
pub struct TaskRetryArgs {
    /// Task ID.
    pub id: String,
}

/// Arguments for retrying multiple persisted tasks.
#[derive(Args)]
pub struct TaskRetryBatchArgs {
    /// Restrict retries to a specific retryable status: failed, cancelled, timed_out.
    #[arg(long)]
    pub status: Option<String>,
    /// Maximum number of retryable tasks to requeue in this invocation.
    #[arg(long)]
    pub limit: Option<usize>,
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

fn parse_retryable_status_filter(status: Option<&str>) -> anyhow::Result<Option<&str>> {
    match status.map(|value| value.trim().to_ascii_lowercase()) {
        None => Ok(None),
        Some(value) if value == "failed" || value == "cancelled" || value == "timed_out" => {
            Ok(Some(match value.as_str() {
                "failed" => "failed",
                "cancelled" => "cancelled",
                "timed_out" => "timed_out",
                _ => unreachable!(),
            }))
        }
        Some(other) => anyhow::bail!(
            "unsupported retry batch status '{other}' (expected failed, cancelled, or timed_out)"
        ),
    }
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

fn orchestrator_config_from_swarm(
    config: &swarm_config::model::OrchestratorConfig,
) -> swarm_orchestrator::OrchestratorConfig {
    swarm_orchestrator::OrchestratorConfig {
        event_channel_capacity: config.event_channel_capacity,
        max_dispatch_per_tick: config.max_dispatch_per_tick,
        default_task_timeout: (config.default_task_timeout_secs != 0)
            .then(|| Duration::from_secs(config.default_task_timeout_secs)),
        max_concurrent_tasks: config.max_concurrent_tasks,
    }
}

#[derive(Debug, Serialize)]
struct ProcessedTaskReport {
    id: String,
    name: String,
    status: String,
    worker: String,
}

#[derive(Debug, Serialize)]
struct TaskProcessSummary {
    available_pending: usize,
    attempted: usize,
    processed: usize,
    completed: usize,
    failed: usize,
    remaining_pending: usize,
    worker_count: usize,
    tasks: Vec<ProcessedTaskReport>,
}

#[derive(Debug, Serialize)]
struct RetriedTaskReport {
    id: String,
    name: String,
    previous_status: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct TaskRetrySummary {
    available_retryable: usize,
    retried: usize,
    remaining_retryable: usize,
    tasks: Vec<RetriedTaskReport>,
}

struct BuiltInTaskWorker {
    descriptor: AgentDescriptor,
}

#[async_trait]
impl Agent for BuiltInTaskWorker {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    async fn execute(&mut self, task: Task) -> SwarmResult<serde_json::Value> {
        let mut required_capabilities = task
            .spec
            .required_capabilities
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        required_capabilities.sort();

        Ok(serde_json::json!({
            "worker": self.descriptor.name,
            "task": task.spec.name,
            "input": task.spec.input,
            "metadata": task.spec.metadata,
            "required_capabilities": required_capabilities,
            "status": "completed",
        }))
    }

    async fn health_check(&self) -> SwarmResult<()> {
        Ok(())
    }
}

fn collect_pending_tasks(mut tasks: Vec<Task>, limit: Option<usize>) -> Vec<Task> {
    tasks.retain(|task| matches!(task.status, TaskStatus::Pending));
    tasks.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    if let Some(limit) = limit {
        tasks.truncate(limit);
    }
    tasks
}

fn combined_capabilities(tasks: &[Task]) -> CapabilitySet {
    tasks
        .iter()
        .flat_map(|task| task.spec.required_capabilities.iter().cloned())
        .collect()
}

fn print_process_summary_text(summary: &TaskProcessSummary) {
    println!("Processed persisted tasks");
    println!("  pending_available: {}", summary.available_pending);
    println!("  attempted:         {}", summary.attempted);
    println!("  processed:         {}", summary.processed);
    println!("  completed:         {}", summary.completed);
    println!("  failed:            {}", summary.failed);
    println!("  remaining_pending: {}", summary.remaining_pending);
    println!("  workers:           {}", summary.worker_count);
    if !summary.tasks.is_empty() {
        println!("  task_results:");
        for task in &summary.tasks {
            println!(
                "    - {} [{}] {} via {}",
                task.id, task.status, task.name, task.worker
            );
        }
    }
}

fn print_retry_summary_text(summary: &TaskRetrySummary) {
    println!("Retried persisted tasks");
    println!("  retryable_available: {}", summary.available_retryable);
    println!("  retried:             {}", summary.retried);
    println!("  remaining_retryable: {}", summary.remaining_retryable);
    if !summary.tasks.is_empty() {
        println!("  task_results:");
        for task in &summary.tasks {
            println!(
                "    - {} [{} -> {}] {}",
                task.id, task.previous_status, task.status, task.name
            );
        }
    }
}

async fn process_pending_tasks(
    args: &TaskProcessArgs,
    config: &SwarmConfig,
) -> anyhow::Result<TaskProcessSummary> {
    if args.workers == 0 {
        anyhow::bail!("workers must be at least 1");
    }

    let store = task_store(config);
    let all_tasks = store.list().await?;
    let available_pending = all_tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Pending))
        .count();
    let pending_tasks = collect_pending_tasks(all_tasks, args.limit);

    if pending_tasks.is_empty() {
        return Ok(TaskProcessSummary {
            available_pending,
            attempted: 0,
            processed: 0,
            completed: 0,
            failed: 0,
            remaining_pending: available_pending,
            worker_count: args.workers,
            tasks: Vec::new(),
        });
    }

    let worker_capabilities = combined_capabilities(&pending_tasks);
    let orchestrator =
        Orchestrator::with_config(orchestrator_config_from_swarm(&config.orchestrator));
    let handle = orchestrator.handle();

    let mut runners = Vec::with_capacity(args.workers);
    for index in 0..args.workers {
        let worker_name = format!("CLI-Worker-{}", index + 1);
        let descriptor = AgentDescriptor::new(
            worker_name.clone(),
            AgentKind::Worker,
            worker_capabilities.clone(),
        );
        handle.register_agent(descriptor.clone())?;
        handle.set_agent_ready(descriptor.id)?;
        runners.push((
            worker_name,
            TaskRunner::new(Box::new(BuiltInTaskWorker { descriptor }), handle.clone()),
        ));
    }

    for task in &pending_tasks {
        handle.import_task_snapshot(task.clone()).await?;
    }

    let mut tasks = Vec::new();
    let mut completed = 0;
    let mut failed = 0;

    while let Some(task_id) = handle.try_schedule_next().await? {
        let scheduled_task = handle.get_task(&task_id)?;
        let assigned_to = match scheduled_task.status {
            TaskStatus::Scheduled { assigned_to } => assigned_to,
            _ => anyhow::bail!("scheduled task {task_id} was not in the scheduled state"),
        };

        let (worker_name, runner) = runners
            .iter_mut()
            .find(|(_, runner)| runner.agent_id() == assigned_to)
            .ok_or_else(|| anyhow::anyhow!("task {task_id} was assigned to unknown worker"))?;

        match runner.run_task(scheduled_task).await {
            Ok(_) => {
                completed += 1;
            }
            Err(error) => {
                failed += 1;
                tracing::warn!(task_id = %task_id, error = %error, "persisted task processing failed");
            }
        }

        let updated_task = handle.get_task(&task_id)?;
        store.record(updated_task.clone()).await?;
        tasks.push(ProcessedTaskReport {
            id: task_id.to_string(),
            name: updated_task.spec.name.clone(),
            status: updated_task.status.label().into(),
            worker: worker_name.clone(),
        });
    }

    let remaining_pending = store
        .list()
        .await?
        .into_iter()
        .filter(|task| matches!(task.status, TaskStatus::Pending))
        .count();

    Ok(TaskProcessSummary {
        available_pending,
        attempted: pending_tasks.len(),
        processed: tasks.len(),
        completed,
        failed,
        remaining_pending,
        worker_count: args.workers,
        tasks,
    })
}

fn collect_retryable_tasks(
    mut tasks: Vec<Task>,
    status: Option<&str>,
    limit: Option<usize>,
) -> Vec<Task> {
    tasks.retain(|task| task.status.can_retry() && status_matches(task, status));
    tasks.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    if let Some(limit) = limit {
        tasks.truncate(limit);
    }
    tasks
}

async fn retry_persisted_tasks(
    args: &TaskRetryBatchArgs,
    config: &SwarmConfig,
) -> anyhow::Result<TaskRetrySummary> {
    let status = parse_retryable_status_filter(args.status.as_deref())?;
    let store = task_store(config);
    let all_tasks = store.list().await?;
    let available_retryable = all_tasks
        .iter()
        .filter(|task| task.status.can_retry() && status_matches(task, status))
        .count();
    let retryable_tasks = collect_retryable_tasks(all_tasks, status, args.limit);

    let mut tasks = Vec::with_capacity(retryable_tasks.len());
    for task in retryable_tasks {
        let previous_status = task.status.label().to_string();
        let retried = store.retry(&task.id).await?;
        tasks.push(RetriedTaskReport {
            id: retried.id.to_string(),
            name: retried.spec.name.clone(),
            previous_status,
            status: retried.status.label().into(),
        });
    }

    let remaining_retryable = store
        .list()
        .await?
        .into_iter()
        .filter(|task| task.status.can_retry() && status_matches(task, status))
        .count();

    Ok(TaskRetrySummary {
        available_retryable,
        retried: tasks.len(),
        remaining_retryable,
        tasks,
    })
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
        TaskSubcommand::Process(args) => {
            let summary = process_pending_tasks(&args, config).await?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&summary)?),
                _ => print_process_summary_text(&summary),
            }
        }
        TaskSubcommand::Retry(args) => {
            let id = parse_task_id(&args.id)?;
            let task = task_store(config).retry(&id).await?;
            println!("Retried task {} (status={})", task.id, task.status.label());
        }
        TaskSubcommand::RetryBatch(args) => {
            let summary = retry_persisted_tasks(&args, config).await?;
            match args.format.as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&summary)?),
                _ => print_retry_summary_text(&summary),
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

    #[tokio::test]
    async fn retry_requeues_failed_task_snapshot() {
        let (_dir, config) = test_config().await;
        let mut task = Task::new(TaskSpec::new("retry-me", serde_json::json!({})));
        task.fail("transient");
        let id = task.id;
        task_store(&config).record(task).await.unwrap();

        run(
            TaskArgs {
                subcommand: TaskSubcommand::Retry(TaskRetryArgs { id: id.to_string() }),
            },
            &config,
        )
        .await
        .unwrap();

        let updated = task_store(&config).get(&id).await.unwrap().unwrap();
        assert_eq!(updated.status.label(), "pending");
    }

    #[tokio::test]
    async fn retry_rejects_completed_task_snapshot() {
        let (_dir, config) = test_config().await;
        let mut task = Task::new(TaskSpec::new("done", serde_json::json!({})));
        task.complete(serde_json::json!({"ok": true}));
        let id = task.id;
        task_store(&config).record(task).await.unwrap();

        let error = run(
            TaskArgs {
                subcommand: TaskSubcommand::Retry(TaskRetryArgs { id: id.to_string() }),
            },
            &config,
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("only failed, cancelled, or timed out tasks can be retried"));
    }

    #[tokio::test]
    async fn retry_batch_requeues_retryable_tasks_by_default() {
        let (_dir, config) = test_config().await;
        let mut failed = Task::new(TaskSpec::new("failed-task", serde_json::json!({})));
        failed.fail("transient");
        let failed_id = failed.id;
        task_store(&config).record(failed).await.unwrap();

        let pending = Task::new(TaskSpec::new("pending-task", serde_json::json!({})));
        let pending_id = pending.id;
        task_store(&config).record(pending).await.unwrap();

        let mut timed_out = Task::new(TaskSpec::new("timed-out-task", serde_json::json!({})));
        timed_out.time_out();
        let timed_out_id = timed_out.id;
        task_store(&config).record(timed_out).await.unwrap();

        let summary = retry_persisted_tasks(
            &TaskRetryBatchArgs {
                status: None,
                limit: None,
                format: "json".into(),
            },
            &config,
        )
        .await
        .unwrap();

        assert_eq!(summary.available_retryable, 2);
        assert_eq!(summary.retried, 2);
        assert_eq!(summary.remaining_retryable, 0);
        assert_eq!(
            task_store(&config)
                .get(&failed_id)
                .await
                .unwrap()
                .unwrap()
                .status
                .label(),
            "pending"
        );
        assert_eq!(
            task_store(&config)
                .get(&timed_out_id)
                .await
                .unwrap()
                .unwrap()
                .status
                .label(),
            "pending"
        );
        assert_eq!(
            task_store(&config)
                .get(&pending_id)
                .await
                .unwrap()
                .unwrap()
                .status
                .label(),
            "pending"
        );
    }

    #[tokio::test]
    async fn retry_batch_respects_status_and_limit_filters() {
        let (_dir, config) = test_config().await;
        for index in 0..2 {
            let mut task = Task::new(TaskSpec::new(
                format!("failed-{index}"),
                serde_json::json!({ "index": index }),
            ));
            task.fail("transient");
            task_store(&config).record(task).await.unwrap();
        }
        let mut cancelled = Task::new(TaskSpec::new("cancelled", serde_json::json!({})));
        cancelled.status = TaskStatus::Cancelled {
            cancelled_at: swarm_core::types::now(),
            reason: Some("operator".into()),
        };
        task_store(&config).record(cancelled).await.unwrap();

        let summary = retry_persisted_tasks(
            &TaskRetryBatchArgs {
                status: Some("failed".into()),
                limit: Some(1),
                format: "text".into(),
            },
            &config,
        )
        .await
        .unwrap();

        assert_eq!(summary.available_retryable, 2);
        assert_eq!(summary.retried, 1);
        assert_eq!(summary.remaining_retryable, 1);
        assert!(summary
            .tasks
            .iter()
            .all(|task| task.previous_status == "failed" && task.status == "pending"));
    }

    #[test]
    fn parse_retryable_status_filter_rejects_non_retryable_status() {
        let error = parse_retryable_status_filter(Some("completed")).unwrap_err();
        assert!(error
            .to_string()
            .contains("unsupported retry batch status 'completed'"));
    }

    fn pending_task_with_capability(name: &str, capability: &str) -> Task {
        let mut spec = TaskSpec::new(name, serde_json::json!({"task": name}));
        spec.required_capabilities.add(Capability::new(capability));
        Task::new(spec)
    }

    #[tokio::test]
    async fn process_completes_pending_persisted_tasks() {
        let (_dir, config) = test_config().await;
        let first = pending_task_with_capability("plan", "planning");
        let second = pending_task_with_capability("analyze", "analysis");
        task_store(&config).record(first.clone()).await.unwrap();
        task_store(&config).record(second.clone()).await.unwrap();

        let summary = process_pending_tasks(
            &TaskProcessArgs {
                limit: None,
                workers: 2,
                format: "json".into(),
            },
            &config,
        )
        .await
        .unwrap();

        assert_eq!(summary.available_pending, 2);
        assert_eq!(summary.processed, 2);
        assert_eq!(summary.completed, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.remaining_pending, 0);

        let tasks = task_store(&config).list().await.unwrap();
        assert_eq!(
            tasks
                .iter()
                .filter(|task| matches!(task.status, TaskStatus::Completed { .. }))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn process_respects_limit_and_leaves_remaining_tasks_pending() {
        let (_dir, config) = test_config().await;
        for index in 0..2 {
            task_store(&config)
                .record(pending_task_with_capability(
                    &format!("task-{index}"),
                    "planning",
                ))
                .await
                .unwrap();
        }

        let summary = process_pending_tasks(
            &TaskProcessArgs {
                limit: Some(1),
                workers: 1,
                format: "text".into(),
            },
            &config,
        )
        .await
        .unwrap();

        assert_eq!(summary.attempted, 1);
        assert_eq!(summary.processed, 1);
        assert_eq!(summary.remaining_pending, 1);
    }

    #[tokio::test]
    async fn process_rejects_zero_workers() {
        let (_dir, config) = test_config().await;

        let error = process_pending_tasks(
            &TaskProcessArgs {
                limit: None,
                workers: 0,
                format: "text".into(),
            },
            &config,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("workers must be at least 1"));
    }

    #[test]
    fn parse_priority_accepts_supported_labels() {
        assert_eq!(parse_priority("low").unwrap(), TaskPriority::Low);
        assert_eq!(parse_priority("normal").unwrap(), TaskPriority::Normal);
        assert_eq!(parse_priority("high").unwrap(), TaskPriority::High);
        assert_eq!(parse_priority("critical").unwrap(), TaskPriority::Critical);
    }
}
