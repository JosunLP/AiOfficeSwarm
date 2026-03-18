//! Priority-ordered task queue.
//!
//! The task queue holds tasks that are waiting to be scheduled. Tasks are
//! ordered by [`TaskPriority`] (descending) and then by submission time
//! (FIFO within the same priority level).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

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

type QueueKey = (i32, Timestamp, u64);

#[derive(Debug, Default)]
struct TaskQueueInner {
    queue: BTreeMap<QueueKey, QueueEntry>,
    index: HashMap<TaskId, QueueKey>,
    next_sequence: u64,
}

/// A thread-safe, priority-ordered queue of pending tasks.
///
/// Internally uses a single mutex guarding both the ordered queue and the
/// ID index. Keys are `(negated_priority, enqueued_at, sequence)` so that
/// higher-priority tasks sort first, tasks at the same priority remain FIFO,
/// and inserts are always unique even when timestamps collide.
#[derive(Clone, Default)]
pub struct TaskQueue {
    inner: Arc<Mutex<TaskQueueInner>>,
}

impl TaskQueue {
    /// Create an empty task queue.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_inner(&self) -> MutexGuard<'_, TaskQueueInner> {
        match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => {
                tracing::error!("Task queue mutex was poisoned; recovering state");
                poisoned.into_inner()
            }
        }
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
        let mut inner = self.lock_inner();
        if inner.index.contains_key(&task.id) {
            return Err(SwarmError::InvalidTaskSpec {
                reason: format!("task {} is already enqueued", task.id),
            });
        }
        let sequence = inner.next_sequence;
        inner.next_sequence += 1;
        let key = (priority_key, enqueued_at, sequence);
        let entry = QueueEntry {
            task: task.clone(),
            enqueued_at,
        };
        inner.queue.insert(key, entry);
        inner.index.insert(task.id, key);
        Ok(())
    }

    /// Peek at the highest-priority pending task without removing it.
    pub fn peek(&self) -> Option<Task> {
        self.lock_inner()
            .queue
            .values()
            .next()
            .map(|e| e.task.clone())
    }

    /// Remove and return the highest-priority pending task.
    pub fn dequeue(&self) -> Option<Task> {
        let mut queue = self.lock_inner();
        let key = queue.queue.keys().next().cloned()?;
        let entry = queue.queue.remove(&key)?;
        queue.index.remove(&entry.task.id);
        Some(entry.task)
    }

    /// Remove a task by ID (e.g., when cancelling before scheduling).
    pub fn remove(&self, task_id: &TaskId) -> SwarmResult<Task> {
        let mut inner = self.lock_inner();
        let key = inner
            .index
            .remove(task_id)
            .ok_or(SwarmError::TaskNotFound { id: *task_id })?;
        inner
            .queue
            .remove(&key)
            .map(|e| e.task)
            .ok_or(SwarmError::TaskNotFound { id: *task_id })
    }

    /// Return the number of tasks currently in the queue.
    pub fn len(&self) -> usize {
        self.lock_inner().queue.len()
    }

    /// Returns `true` if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.lock_inner().queue.is_empty()
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

    #[test]
    fn duplicate_task_id_is_rejected_without_mutating_queue() {
        let queue = TaskQueue::new();
        let task = make_task(TaskPriority::Normal);
        let duplicate = task.clone();
        let id = task.id;

        queue.enqueue(task).unwrap();
        let err = queue
            .enqueue(duplicate)
            .expect_err("duplicate task ID should be rejected");

        assert!(matches!(err, SwarmError::InvalidTaskSpec { .. }));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.peek().unwrap().id, id);
        assert_eq!(queue.dequeue().unwrap().id, id);
        assert!(queue.is_empty());
    }

    #[test]
    fn remove_one_task_does_not_drop_another_with_same_timestamp() {
        let queue = TaskQueue::new();
        let t1 = make_task(TaskPriority::Normal);
        let t2 = make_task(TaskPriority::Normal);
        let id2 = t2.id;

        queue.enqueue(t1).unwrap();
        queue.enqueue(t2).unwrap();

        let first = queue.dequeue().unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.peek().unwrap().id, id2);
        assert_ne!(first.id, queue.peek().unwrap().id);
    }

    #[test]
    fn recovers_from_poisoned_mutex() {
        let queue = TaskQueue::new();
        let poisoned = queue.clone();

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoned.inner.lock().unwrap();
            panic!("poison task queue mutex");
        }));

        queue.enqueue(make_task(TaskPriority::Normal)).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
    }
}
