//! # swarm-orchestrator
//!
//! The orchestration engine for the AiOfficeSwarm framework.
//!
//! This crate provides:
//!
//! - [`AgentRegistry`]: Tracks all registered agents and their runtime state.
//! - [`TaskQueue`]: A priority-ordered queue of pending tasks awaiting scheduling.
//! - [`Scheduler`]: Matches tasks to available agents based on capabilities.
//! - [`Orchestrator`]: The top-level control loop that ties everything together.
//! - [`SupervisionManager`]: Maintains the agent supervision tree and handles
//!   fault escalation.
//!
//! ## Architecture
//! The orchestrator follows a control-plane pattern: it maintains the *desired
//! state* (registered agents, submitted tasks) and continuously reconciles it
//! with the *actual state* (agent statuses, task outcomes) via an async
//! reconciliation loop.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod orchestrator;
pub mod registry;
pub mod scheduler;
pub mod supervision;
pub mod task_queue;
pub mod task_store;

pub use orchestrator::{Orchestrator, OrchestratorConfig, OrchestratorHandle};
pub use registry::{AgentRecord, AgentRegistry};
pub use scheduler::{Scheduler, SchedulingDecision};
pub use supervision::SupervisionManager;
pub use task_queue::TaskQueue;
pub use task_store::{FileTaskStore, InMemoryTaskStore, TaskStore};
