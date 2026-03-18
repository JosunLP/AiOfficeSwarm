//! Retry executor: applies configurable retry policies to fallible async
//! operations with exponential backoff and optional jitter.

use std::future::Future;
use std::time::Duration;

use swarm_core::error::SwarmResult;
use swarm_core::types::RetryPolicy;

/// Executes an async closure with automatic retries according to a
/// [`RetryPolicy`].
pub struct RetryExecutor {
    policy: RetryPolicy,
    /// Whether to add random jitter to delays (recommended in production to
    /// prevent thundering-herd problems).
    jitter: bool,
}

impl RetryExecutor {
    /// Create an executor using the given policy, with jitter enabled.
    pub fn new(policy: RetryPolicy) -> Self {
        Self {
            policy,
            jitter: true,
        }
    }

    /// Create an executor with jitter disabled (useful in tests for
    /// deterministic timing).
    pub fn without_jitter(policy: RetryPolicy) -> Self {
        Self {
            policy,
            jitter: false,
        }
    }

    fn delay_before_retry(&self, attempt: u32) -> Duration {
        let delay = self.policy.delay_for_attempt(attempt);
        if self.jitter {
            add_jitter(delay).min(self.policy.max_delay)
        } else {
            delay
        }
    }

    /// Execute `f` with automatic retries.
    ///
    /// Only [`SwarmError::is_retryable`] errors trigger a retry. Non-retryable
    /// errors (e.g., policy violations) are returned immediately.
    pub async fn execute<F, Fut, T>(&self, mut f: F) -> SwarmResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = SwarmResult<T>>,
    {
        let mut attempt = 0u32;
        loop {
            match f().await {
                Ok(value) => return Ok(value),
                Err(e) if !e.is_retryable() || attempt >= self.policy.max_attempts => {
                    return Err(e);
                }
                Err(e) => {
                    attempt += 1;
                    let delay = self.delay_before_retry(attempt);
                    tracing::warn!(
                        attempt = attempt,
                        max_attempts = self.policy.max_attempts,
                        delay_ms = delay.as_millis(),
                        error = e.to_string(),
                        "Retrying after error"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

/// Add up to 10% random jitter to a duration.
fn add_jitter(duration: Duration) -> Duration {
    use rand::Rng;
    let jitter_pct = rand::thread_rng().gen_range(0.0_f64..0.1);
    let jitter_secs = duration.as_secs_f64() * jitter_pct;
    duration + Duration::from_secs_f64(jitter_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_core::error::SwarmError;
    use swarm_core::identity::TaskId;
    use swarm_core::types::{RetryPolicy, RetryStrategy};

    fn no_retry_policy() -> RetryPolicy {
        RetryPolicy::no_retry()
    }

    fn fast_retry_policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 2,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            strategy: RetryStrategy::Fixed,
        }
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt() {
        let executor = RetryExecutor::without_jitter(no_retry_policy());
        let result = executor
            .execute(|| async { Ok::<i32, SwarmError>(42) })
            .await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let executor = RetryExecutor::without_jitter(fast_retry_policy());
        let mut calls = 0u32;
        let result = executor
            .execute(|| {
                calls += 1;
                async {
                    Err::<i32, SwarmError>(SwarmError::PolicyViolation {
                        policy_id: swarm_core::identity::PolicyId::new(),
                        action: "test".into(),
                        reason: "blocked".into(),
                    })
                }
            })
            .await;
        assert!(result.is_err());
        assert_eq!(calls, 1, "Non-retryable errors must not be retried");
    }

    #[tokio::test]
    async fn retries_transient_errors() {
        let executor = RetryExecutor::without_jitter(fast_retry_policy());
        let mut calls = 0u32;
        let result = executor
            .execute(|| {
                calls += 1;
                let c = calls;
                async move {
                    if c < 3 {
                        Err::<i32, SwarmError>(SwarmError::TaskTimeout {
                            id: TaskId::new(),
                            elapsed_ms: 100,
                        })
                    } else {
                        Ok(99)
                    }
                }
            })
            .await;
        assert_eq!(result.unwrap(), 99);
        assert_eq!(calls, 3);
    }

    #[test]
    fn jittered_delay_respects_max_delay_cap() {
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(10),
            strategy: RetryStrategy::ExponentialBackoff { multiplier: 10.0 },
        };
        let executor = RetryExecutor::new(policy.clone());

        for _ in 0..32 {
            assert!(executor.delay_before_retry(2) <= policy.max_delay);
        }
    }
}
