//! Circuit breaker: prevents cascading failures by temporarily blocking calls
//! to a failing component.
//!
//! ## State machine
//! ```text
//! Closed ──(failure threshold reached)──► Open
//!   ▲                                       │
//!   │                                  (timeout)
//!   │                                       ▼
//!   └────(probe succeeds)────────── HalfOpen
//! ```
//!
//! - **Closed**: Normal operation. Failures are counted.
//! - **Open**: All calls are rejected immediately (fail fast).
//! - **HalfOpen**: A single probe call is allowed. Success closes the circuit;
//!   failure re-opens it and resets the timeout.

use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};
use swarm_core::error::{SwarmError, SwarmResult};

/// The current state of a circuit breaker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation; failures are counted.
    Closed,
    /// Circuit is open; all calls are rejected.
    Open {
        /// When the circuit was opened.
        opened_at: DateTime<Utc>,
        /// When the circuit will transition to `HalfOpen` for a probe.
        retry_after: DateTime<Utc>,
    },
    /// One probe call is allowed; outcome determines next state.
    HalfOpen,
}

/// Configuration for a circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before the circuit opens.
    pub failure_threshold: u32,
    /// How long the circuit stays open before allowing a probe (seconds).
    pub open_duration_secs: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration_secs: 30,
        }
    }
}

struct CircuitBreakerInner {
    state: CircuitState,
    consecutive_failures: u32,
    config: CircuitBreakerConfig,
}

/// A thread-safe circuit breaker.
///
/// Wrap calls to a component with [`CircuitBreaker::call`] to automatically
/// open the circuit when too many consecutive failures occur.
#[derive(Clone)]
pub struct CircuitBreaker {
    name: String,
    inner: Arc<Mutex<CircuitBreakerInner>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given name and default config.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_config(name, CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with explicit configuration.
    pub fn with_config(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            inner: Arc::new(Mutex::new(CircuitBreakerInner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                config,
            })),
        }
    }

    /// Returns the current state of the circuit.
    pub fn state(&self) -> CircuitState {
        self.inner.lock().unwrap().state.clone()
    }

    /// Returns `true` if calls are currently allowed.
    pub fn is_closed(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        match &inner.state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open { retry_after, .. } => Utc::now() >= *retry_after,
        }
    }

    /// Attempt to acquire a permit for a call.
    ///
    /// Returns `Err` if the circuit is open and the retry window has not
    /// elapsed. If the retry window *has* elapsed, transitions to `HalfOpen`
    /// and allows the probe call.
    pub fn acquire(&self) -> SwarmResult<()> {
        let mut inner = self.inner.lock().unwrap();
        match &inner.state {
            CircuitState::Closed => Ok(()),
            CircuitState::HalfOpen => Ok(()),
            CircuitState::Open { retry_after, .. } => {
                if Utc::now() >= *retry_after {
                    tracing::info!(circuit = self.name, "Circuit transitioning to HalfOpen");
                    inner.state = CircuitState::HalfOpen;
                    Ok(())
                } else {
                    Err(SwarmError::Internal {
                        reason: format!(
                            "circuit '{}' is open; retry after {:?}",
                            self.name, retry_after
                        ),
                    })
                }
            }
        }
    }

    /// Record a successful call. Transitions `HalfOpen` → `Closed` and
    /// resets the failure counter.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.state == CircuitState::HalfOpen {
            tracing::info!(circuit = self.name, "Circuit closed after successful probe");
        }
        inner.state = CircuitState::Closed;
        inner.consecutive_failures = 0;
    }

    /// Record a failed call. May transition `Closed` → `Open` or
    /// `HalfOpen` → `Open`.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.consecutive_failures += 1;
        let should_open = match &inner.state {
            CircuitState::Closed => inner.consecutive_failures >= inner.config.failure_threshold,
            CircuitState::HalfOpen => true,
            CircuitState::Open { .. } => false,
        };
        if should_open {
            let opened_at = Utc::now();
            let retry_after = opened_at
                + chrono::Duration::seconds(inner.config.open_duration_secs as i64);
            tracing::warn!(
                circuit = self.name,
                failures = inner.consecutive_failures,
                "Circuit opened due to repeated failures"
            );
            inner.state = CircuitState::Open { opened_at, retry_after };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cb(threshold: u32) -> CircuitBreaker {
        CircuitBreaker::with_config("test", CircuitBreakerConfig {
            failure_threshold: threshold,
            open_duration_secs: 60,
        })
    }

    #[test]
    fn starts_closed() {
        let cb = make_cb(3);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.acquire().is_ok());
    }

    #[test]
    fn opens_after_threshold_failures() {
        let cb = make_cb(3);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.acquire().is_ok()); // still closed after 2 failures
        cb.record_failure(); // 3rd failure → open
        assert!(cb.acquire().is_err());
    }

    #[test]
    fn success_resets_failure_count() {
        let cb = make_cb(3);
        cb.record_failure();
        cb.record_failure();
        cb.record_success(); // reset
        cb.record_failure(); // 1st failure again
        assert!(cb.acquire().is_ok());
    }
}
