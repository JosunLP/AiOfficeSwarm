//! # swarm-telemetry
//!
//! Observability infrastructure for the AiOfficeSwarm framework.
//!
//! This crate provides:
//!
//! - [`init_tracing`]: Configures the global `tracing` subscriber based on
//!   [`TelemetryConfig`].
//! - [`Metrics`]: In-process atomic counters for key framework events.
//! - [`AuditLogger`]: Structured audit log that records security-sensitive
//!   operations.
//!
//! ## Design
//! The telemetry layer uses the `tracing` crate as its foundation. All
//! structured events emitted by the framework use `tracing` macros
//! (`info!`, `warn!`, `error!`, `debug!`). The subscriber is configured
//! once at startup via [`init_tracing`].
//!
//! Metrics are intentionally kept as in-process atomics for the MVP.
//! Future versions may export to Prometheus or OpenTelemetry.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod audit;
pub mod metrics;
pub mod tracing_init;

pub use audit::{AuditEntry, AuditLogger};
pub use metrics::Metrics;
pub use tracing_init::init_tracing;
