//! Priority-ordered task queue.
//!
//! The task queue holds tasks that are waiting to be scheduled. Tasks are
//! ordered by [`TaskPriority`] (descending) and then by submission time
//! (FIFO within the same priority level).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use swarm_core::{
    error::{SwarmError, SwarmResult},
    identity::TaskId,
    task::{Task, TaskStatus},
    types::Timestamp,
};

/// An entry in the task queue with ordering metadata.
#[derive(Debug, Clone)]
struct QueueEntry {
    task: Task,
    #[allow(dead_code)]
    enqueued_at: Timestamp,
}

/// A thread-safe, priority-ordered queue of pending tasks.
///
/// Internally uses a `BTreeMap` keyed on `(negated_priority, enqueued_at)` so
/// that higher-priority tasks sort first, and tasks at the same priority are
/// FIFO by submission time. A parallel `HashMap<TaskId, key>` provides O(1)
/// lookup by task ID for cancellation.
#[derive(Clone, Default)]
pub struct TaskQueue {
    // Key: (negated priority as i32, enqueued_at) for natural sort order
    inner: Arc<Mutex<BTreeMap<(i32, Timestamp), QueueEntry>>>,
    // Parallel index for O(1) lookup by TaskId (HashMap, not BTreeMap, since
    // TaskId doesn't implement Ord)
    index: Arc<Mutex<HashMap<TaskId, (i32, Timestamp)>>>,
}

impl TaskQueue {
    /// Create an empty task queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a task. The task must be in the `Pending` state.
    pub fn enqueue(&self, task: Task) -> SwarmResult<()> {
        if !matches!(task.status, TaskStatus::Pending) {
            return Err(SwarmError::InvalidTaskSpec {
                reason: format!(
                    "only Pending tasks can be enqueued; got {}",
                    task.status.label()
                ),
            });
        }
        let priority_key = -(task.spec.priority as i32);
        let enqueued_at = swarm_core::types::now();
        let key = (priority_key, enqueued_at);
        let entry = QueueEntry { task: task.clone(), enqueued_at };
        self.inner.lock().unwrap().insert(key, entry);
        self.index.lock().unwrap().insert(task.id, key);
        Ok(())
    }

    /// Peek at the highest-priority pending task without removing it.
    pub fn peek(&self) -> Option<Task> {
        self.inner
            .lock()
            .unwrap()
            .values()
            .next()
            .map(|e| e.task.clone())
    }

    /// Remove and return the highest-priority pending task.
    pub fn dequeue(&self) -> Option<Task> {
        let mut queue = self.inner.lock().unwrap();
        let key = queue.keys().next().cloned()?;
        let entry = queue.remove(&key)?;
        self.index.lock().unwrap().remove(&entry.task.id);
        Some(entry.task)
    }

    /// Remove a task by ID (e.g., when cancelling before scheduling).
    pub fn remove(&self, task_id: &TaskId) -> SwarmResult<Task> {
        let key = self
            .index
            .lock()
            .unwrap()
            .remove(task_id)
            .ok_or_else(|| SwarmError::TaskNotFound { id: *task_id })?;
        self.inner
            .lock()
            .unwrap()
            .remove(&key)
            .map(|e| e.task)
            .ok_or_else(|| SwarmError::TaskNotFound { id: *task_id })
    }

    /// Return the number of tasks currently in the queue.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Returns `true` if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::task::{TaskPriority, TaskSpec};

    fn make_task(priority: TaskPriority) -> Task {
        let mut spec = TaskSpec::new("test", serde_json::json!({}));
        spec.priority = priority;
        Task::new(spec)
    }

    #[test]
    fn enqueue_and_dequeue_fifo_same_priority() {
        let queue = TaskQueue::new();
        let t1 = make_task(TaskPriority::Normal);
        let t2 = make_task(TaskPriority::Normal);
        let id1 = t1.id;

        queue.enqueue(t1).unwrap();
        queue.enqueue(t2).unwrap();

        let dequeued = queue.dequeue().unwrap();
        assert_eq!(dequeued.id, id1, "FIFO within same priority");
    }

    #[test]
    fn high_priority_dequeued_before_normal() {
        let queue = TaskQueue::new();
        let normal = make_task(TaskPriority::Normal);
        let high = make_task(TaskPriority::High);
        let high_id = high.id;

        queue.enqueue(normal).unwrap();
        queue.enqueue(high).unwrap();

        let first = queue.dequeue().unwrap();
        assert_eq!(first.id, high_id);
    }

    #[test]
    fn remove_by_id() {
        let queue = TaskQueue::new();
        let task = make_task(TaskPriority::Normal);
        let id = task.id;
        queue.enqueue(task).unwrap();
        queue.remove(&id).expect("should remove");
        assert!(queue.is_empty());
    }

    #[test]
    fn queue_len() {
        let queue = TaskQueue::new();
        assert_eq!(queue.len(), 0);
        queue.enqueue(make_task(TaskPriority::Normal)).unwrap();
        queue.enqueue(make_task(TaskPriority::High)).unwrap();
        assert_eq!(queue.len(), 2);
    }
}
