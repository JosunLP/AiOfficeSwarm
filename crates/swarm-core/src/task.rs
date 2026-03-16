//! Task domain model.
//!
//! A [`Task`] represents a discrete unit of work that the orchestrator assigns
//! to an agent. Tasks carry a specification ([`TaskSpec`]), track their own
//! lifecycle via [`TaskStatus`], and are prioritized via [`TaskPriority`].
//!
//! ## Lifecycle
//! ```text
//! Pending → Scheduled → Running → { Completed | Failed | Cancelled | TimedOut }
//! ```
//! Tasks in a terminal state are immutable and may be archived.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;
use crate::identity::{AgentId, TaskId};
use crate::types::{Metadata, RetryPolicy};

/// The specification describing *what* a task should do.
///
/// `TaskSpec` is the declarative intent of the task. It contains the domain
/// payload (`input`) plus resource and routing hints used by the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    /// Human-readable name for display and logging purposes.
    pub name: String,
    /// Domain-specific input payload (free-form JSON).
    pub input: serde_json::Value,
    /// Capabilities that the assigned agent must possess.
    pub required_capabilities: CapabilitySet,
    /// Execution priority hint for the scheduler.
    pub priority: TaskPriority,
    /// Optional maximum wall-clock time for this task.
    pub timeout: Option<Duration>,
    /// Retry policy to apply on transient failures.
    pub retry_policy: RetryPolicy,
    /// Arbitrary metadata labels.
    pub metadata: Metadata,
}

impl TaskSpec {
    /// Create a minimal task specification with sensible defaults.
    pub fn new(name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            input,
            required_capabilities: CapabilitySet::new(),
            priority: TaskPriority::Normal,
            timeout: Some(Duration::from_secs(300)),
            retry_policy: RetryPolicy::default(),
            metadata: Metadata::new(),
        }
    }
}

/// The execution priority of a task.
///
/// Higher-priority tasks are scheduled before lower-priority tasks when the
/// scheduler has a choice between candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    /// Lowest priority — background or maintenance tasks.
    Low = 0,
    /// Default priority for most tasks.
    Normal = 1,
    /// High-priority tasks that should be scheduled ahead of `Normal`.
    High = 2,
    /// Critical tasks — must be executed as soon as possible.
    Critical = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// The current lifecycle state of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task has been accepted but not yet assigned to an agent.
    Pending,
    /// Task has been assigned to an agent and is waiting to start.
    Scheduled {
        /// The agent that has been assigned this task.
        assigned_to: AgentId,
    },
    /// Task is actively executing on an agent.
    Running {
        /// The agent currently executing this task.
        executing_on: AgentId,
        /// When execution began.
        started_at: DateTime<Utc>,
    },
    /// Task completed successfully.
    Completed {
        /// When the task completed.
        completed_at: DateTime<Utc>,
        /// The output produced by the agent (free-form JSON).
        output: serde_json::Value,
    },
    /// Task failed after exhausting retries.
    Failed {
        /// When the failure was recorded.
        failed_at: DateTime<Utc>,
        /// Human-readable description of the failure.
        reason: String,
        /// Number of attempts that were made.
        attempts: u32,
    },
    /// Task was explicitly cancelled by an operator or parent agent.
    Cancelled {
        /// When cancellation was requested.
        cancelled_at: DateTime<Utc>,
        /// Optional reason for cancellation.
        reason: Option<String>,
    },
    /// Task exceeded its configured timeout.
    TimedOut {
        /// When the timeout was detected.
        timed_out_at: DateTime<Utc>,
    },
}

impl TaskStatus {
    /// Returns `true` if the task has reached a terminal state (no further
    /// transitions are possible).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed { .. }
                | TaskStatus::Failed { .. }
                | TaskStatus::Cancelled { .. }
                | TaskStatus::TimedOut { .. }
        )
    }

    /// Returns a short, stable string label suitable for metrics and logging.
    pub fn label(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Scheduled { .. } => "scheduled",
            TaskStatus::Running { .. } => "running",
            TaskStatus::Completed { .. } => "completed",
            TaskStatus::Failed { .. } => "failed",
            TaskStatus::Cancelled { .. } => "cancelled",
            TaskStatus::TimedOut { .. } => "timed_out",
        }
    }
}

/// A fully instantiated task including its spec, status, and audit timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier.
    pub id: TaskId,
    /// The specification describing what this task should do.
    pub spec: TaskSpec,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// When the task was first submitted to the orchestrator.
    pub created_at: DateTime<Utc>,
    /// When the task record was last updated.
    pub updated_at: DateTime<Utc>,
    /// Number of execution attempts so far (including the current one).
    pub attempt_count: u32,
}

impl Task {
    /// Create a new task in the `Pending` state.
    pub fn new(spec: TaskSpec) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            spec,
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
            attempt_count: 0,
        }
    }

    /// Transition the task to `Scheduled`, assigning it to the given agent.
    ///
    /// # Errors
    /// Returns the current status if the transition is invalid (task must be
    /// in `Pending` state to be scheduled).
    pub fn schedule(&mut self, agent_id: AgentId) -> Result<(), &TaskStatus> {
        if !matches!(self.status, TaskStatus::Pending) {
            return Err(&self.status);
        }
        self.status = TaskStatus::Scheduled { assigned_to: agent_id };
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Transition the task to `Running`.
    ///
    /// # Errors
    /// Returns the current status if the task is not in the `Scheduled` state.
    pub fn start_running(&mut self, agent_id: AgentId) -> Result<(), &TaskStatus> {
        if !matches!(self.status, TaskStatus::Scheduled { .. }) {
            return Err(&self.status);
        }
        self.attempt_count += 1;
        self.status = TaskStatus::Running {
            executing_on: agent_id,
            started_at: Utc::now(),
        };
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Mark the task as completed successfully.
    pub fn complete(&mut self, output: serde_json::Value) {
        let now = Utc::now();
        self.status = TaskStatus::Completed {
            completed_at: now,
            output,
        };
        self.updated_at = now;
    }

    /// Mark the task as failed.
    pub fn fail(&mut self, reason: impl Into<String>) {
        let now = Utc::now();
        self.status = TaskStatus::Failed {
            failed_at: now,
            reason: reason.into(),
            attempts: self.attempt_count,
        };
        self.updated_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::AgentId;

    fn make_task() -> Task {
        let spec = TaskSpec::new("test-task", serde_json::json!({"prompt": "hello"}));
        Task::new(spec)
    }

    #[test]
    fn task_initial_status_is_pending() {
        let task = make_task();
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.attempt_count, 0);
    }

    #[test]
    fn task_lifecycle_happy_path() {
        let mut task = make_task();
        let agent = AgentId::new();

        task.schedule(agent).expect("schedule should succeed");
        assert!(matches!(task.status, TaskStatus::Scheduled { .. }));

        task.start_running(agent).expect("start_running should succeed");
        assert!(matches!(task.status, TaskStatus::Running { .. }));
        assert_eq!(task.attempt_count, 1);

        task.complete(serde_json::json!({"result": "ok"}));
        assert!(task.status.is_terminal());
        assert_eq!(task.status.label(), "completed");
    }

    #[test]
    fn task_schedule_rejects_invalid_state() {
        let mut task = make_task();
        let agent = AgentId::new();
        task.schedule(agent).unwrap();
        // Second schedule should fail
        assert!(task.schedule(agent).is_err());
    }

    #[test]
    fn task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
    }
}
