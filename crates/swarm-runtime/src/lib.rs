//! # swarm-runtime
//!
//! Execution runtime for the AiOfficeSwarm framework.
//!
//! This crate bridges the orchestrator (which *decides* what to run) and
//! agent implementations (which *do* the work). It provides:
//!
//! - [`TaskRunner`]: Wraps an [`Agent`] and drives task execution with
//!   retry, timeout, and circuit-breaker support.
//! - [`CircuitBreaker`]: Prevents repeated calls to a failing agent by
//!   temporarily opening the circuit after a threshold of failures.
//! - [`RetryExecutor`]: Applies configurable retry policies to fallible
//!   async operations.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod circuit_breaker;
pub mod retry;
pub mod runner;

pub use circuit_breaker::{CircuitBreaker, CircuitState};
pub use retry::RetryExecutor;
pub use runner::{ProviderRoutingOptions, TaskExecutionContext, TaskRunner};
