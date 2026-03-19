//! Shared primitive types used throughout the framework.
//!
//! This module provides lightweight value types that do not belong to a single
//! domain module but are referenced by multiple modules.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A UTC timestamp alias for clarity at call sites.
pub type Timestamp = DateTime<Utc>;

/// Returns the current UTC timestamp.
pub fn now() -> Timestamp {
    Utc::now()
}

/// Arbitrary key-value metadata attached to domain objects.
///
/// Metadata is intentionally untyped (string → string) so that agents,
/// plugins, and operators can attach domain-specific labels without requiring
/// schema changes to core types.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Metadata(pub HashMap<String, String>);

impl Metadata {
    /// Create empty metadata.
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Insert or update a key-value pair.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    /// Retrieve a metadata value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    /// Returns `true` if no metadata entries are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Resource usage limits for an agent or task.
///
/// Enforced by the runtime layer. Exceeding a limit triggers a
/// [`SwarmError::AgentResourceExceeded`](crate::error::SwarmError::AgentResourceExceeded).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceLimits {
    /// Maximum wall-clock time allowed for a single task execution.
    pub max_execution_time: Option<Duration>,
    /// Maximum number of concurrent sub-tasks an agent may spawn.
    pub max_concurrency: Option<usize>,
    /// Maximum memory (in bytes) the agent process may use (advisory).
    pub max_memory_bytes: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_execution_time: Some(Duration::from_secs(300)), // 5 minutes
            max_concurrency: Some(10),
            max_memory_bytes: None,
        }
    }
}

impl ResourceLimits {
    /// Create permissive (effectively unlimited) resource limits.
    ///
    /// Use this sparingly — prefer explicit limits in production deployments.
    pub fn unlimited() -> Self {
        Self {
            max_execution_time: None,
            max_concurrency: None,
            max_memory_bytes: None,
        }
    }
}

/// Retry policy controlling how the orchestrator handles transient failures.
///
/// The orchestrator applies exponential backoff with optional jitter when
/// `strategy` is [`RetryStrategy::ExponentialBackoff`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_attempts: u32,
    /// Base delay between attempts.
    pub initial_delay: Duration,
    /// Maximum delay cap (prevents unbounded backoff).
    pub max_delay: Duration,
    /// Backoff strategy variant.
    pub strategy: RetryStrategy,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            strategy: RetryStrategy::ExponentialBackoff { multiplier: 2.0 },
        }
    }
}

impl RetryPolicy {
    /// Create a policy that never retries.
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 0,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            strategy: RetryStrategy::Fixed,
        }
    }

    /// Compute the delay before the given attempt number using the configured strategy.
    ///
    /// `attempt` is 1-indexed (attempt 1 is the first retry).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let base = match self.strategy {
            RetryStrategy::Fixed => self.initial_delay,
            RetryStrategy::ExponentialBackoff { multiplier } => {
                if !multiplier.is_finite() || multiplier <= 0.0 {
                    return self.max_delay;
                }

                let factor = multiplier.powi((attempt - 1) as i32);
                if !factor.is_finite() || factor <= 0.0 {
                    return self.max_delay;
                }

                let capped_secs =
                    (self.initial_delay.as_secs_f64() * factor).min(self.max_delay.as_secs_f64());

                if !capped_secs.is_finite() {
                    return self.max_delay;
                }

                Duration::from_secs_f64(capped_secs)
            }
        };
        base.min(self.max_delay)
    }
}

/// The mathematical strategy used to compute inter-retry delays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RetryStrategy {
    /// All retries use the same fixed delay.
    Fixed,
    /// Each retry delay is multiplied by `multiplier` relative to the previous.
    ExponentialBackoff {
        /// The multiplicative factor applied per attempt.
        multiplier: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_insert_and_get() {
        let mut m = Metadata::new();
        m.insert("env", "production");
        assert_eq!(m.get("env"), Some("production"));
        assert_eq!(m.get("missing"), None);
    }

    #[test]
    fn retry_policy_exponential_backoff() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            strategy: RetryStrategy::ExponentialBackoff { multiplier: 2.0 },
        };
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
    }

    #[test]
    fn retry_policy_max_delay_cap() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(3),
            strategy: RetryStrategy::ExponentialBackoff { multiplier: 10.0 },
        };
        // After a few attempts the delay should be capped at max_delay.
        assert!(policy.delay_for_attempt(3) <= Duration::from_secs(3));
    }

    #[test]
    fn retry_policy_invalid_multiplier_falls_back_to_max_delay() {
        for multiplier in [f64::NAN, f64::INFINITY, 0.0, -2.0] {
            let policy = RetryPolicy {
                max_attempts: 5,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(3),
                strategy: RetryStrategy::ExponentialBackoff { multiplier },
            };

            assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(3));
        }
    }

    #[test]
    fn retry_policy_overflowing_backoff_is_capped_without_panicking() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(3),
            strategy: RetryStrategy::ExponentialBackoff {
                multiplier: f64::MAX,
            },
        };

        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(3));
    }

    #[test]
    fn resource_limits_default_has_sensible_values() {
        let limits = ResourceLimits::default();
        assert!(limits.max_execution_time.is_some());
        assert!(limits.max_concurrency.is_some());
    }
}
