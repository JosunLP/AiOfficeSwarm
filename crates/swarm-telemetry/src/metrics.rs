//! In-process atomic metrics counters.
//!
//! [`Metrics`] provides lightweight, lock-free counters for key framework
//! events. It is designed to be shared across threads via `Arc<Metrics>`.
//!
//! ## Future work
//! Future versions will expose these counters via a Prometheus exporter or
//! OpenTelemetry metrics SDK.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Shared, thread-safe metrics counters.
#[derive(Default)]
pub struct Metrics {
    /// Number of tasks submitted to the orchestrator.
    pub tasks_submitted: AtomicU64,
    /// Number of tasks that completed successfully.
    pub tasks_completed: AtomicU64,
    /// Number of tasks that failed.
    pub tasks_failed: AtomicU64,
    /// Number of tasks that were cancelled.
    pub tasks_cancelled: AtomicU64,
    /// Number of agents currently registered.
    pub agents_registered: AtomicU64,
    /// Number of policy evaluations performed.
    pub policy_evaluations: AtomicU64,
    /// Number of policy denials.
    pub policy_denials: AtomicU64,
    /// Number of plugin invocations.
    pub plugin_invocations: AtomicU64,
}

impl Metrics {
    /// Create a new zeroed metrics instance.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Increment `tasks_submitted` by 1.
    pub fn inc_tasks_submitted(&self) {
        self.tasks_submitted.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `tasks_completed` by 1.
    pub fn inc_tasks_completed(&self) {
        self.tasks_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `tasks_failed` by 1.
    pub fn inc_tasks_failed(&self) {
        self.tasks_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `tasks_cancelled` by 1.
    pub fn inc_tasks_cancelled(&self) {
        self.tasks_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `agents_registered` by 1.
    pub fn inc_agents_registered(&self) {
        self.agents_registered.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement `agents_registered` by 1 (when an agent is deregistered).
    pub fn dec_agents_registered(&self) {
        self.agents_registered.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment `policy_evaluations` by 1.
    pub fn inc_policy_evaluations(&self) {
        self.policy_evaluations.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `policy_denials` by 1.
    pub fn inc_policy_denials(&self) {
        self.policy_denials.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment `plugin_invocations` by 1.
    pub fn inc_plugin_invocations(&self) {
        self.plugin_invocations.fetch_add(1, Ordering::Relaxed);
    }

    /// Return a snapshot of all counters as a simple struct.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            tasks_submitted: self.tasks_submitted.load(Ordering::Relaxed),
            tasks_completed: self.tasks_completed.load(Ordering::Relaxed),
            tasks_failed: self.tasks_failed.load(Ordering::Relaxed),
            tasks_cancelled: self.tasks_cancelled.load(Ordering::Relaxed),
            agents_registered: self.agents_registered.load(Ordering::Relaxed),
            policy_evaluations: self.policy_evaluations.load(Ordering::Relaxed),
            policy_denials: self.policy_denials.load(Ordering::Relaxed),
            plugin_invocations: self.plugin_invocations.load(Ordering::Relaxed),
        }
    }
}

/// A point-in-time snapshot of all metrics counters.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    /// Tasks submitted.
    pub tasks_submitted: u64,
    /// Tasks completed.
    pub tasks_completed: u64,
    /// Tasks failed.
    pub tasks_failed: u64,
    /// Tasks cancelled.
    pub tasks_cancelled: u64,
    /// Agents currently registered.
    pub agents_registered: u64,
    /// Policy evaluations performed.
    pub policy_evaluations: u64,
    /// Policy denials issued.
    pub policy_denials: u64,
    /// Plugin invocations performed.
    pub plugin_invocations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_increment_and_snapshot() {
        let m = Metrics::new();
        m.inc_tasks_submitted();
        m.inc_tasks_submitted();
        m.inc_tasks_completed();
        let snap = m.snapshot();
        assert_eq!(snap.tasks_submitted, 2);
        assert_eq!(snap.tasks_completed, 1);
        assert_eq!(snap.tasks_failed, 0);
    }
}
