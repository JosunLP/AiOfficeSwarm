//! # swarm-core
//!
//! The foundational crate for the AiOfficeSwarm framework. This crate defines:
//!
//! - **Domain types**: `AgentId`, `TaskId`, `PolicyId`, and all core identifiers.
//! - **Core traits**: `Agent`, `Task`, `Policy`, `Capability`, and more.
//! - **Error model**: A unified, structured error hierarchy used across the workspace.
//! - **Event model**: The event types that flow through the swarm event bus.
//! - **Status/lifecycle types**: Agent and task lifecycle state machines.
//! - **RBAC primitives**: Roles, permissions, and authorization contracts.
//!
//! All other crates in the workspace depend on `swarm-core`. It provides the
//! shared domain model and contracts, while avoiding dependencies on other
//! workspace crates or runtime service integrations.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod agent;
pub mod capability;
pub mod error;
pub mod event;
pub mod identity;
pub mod policy;
pub mod rbac;
pub mod task;
pub mod types;

// Re-export the most commonly needed types at the crate root for ergonomic use.
pub use agent::{
    Agent, AgentKind, AgentStatus, LearningPolicyRef, MemoryAccessProfileRef,
    OperationalConstraints, ProviderPreferences, SupervisionTree, ToolPermissions, TrustLevel,
};
pub use capability::{Capability, CapabilitySet};
pub use error::{SwarmError, SwarmResult};
pub use event::{Event, EventEnvelope, EventKind};
pub use identity::{AgentId, PluginId, PolicyId, TaskId, TenantId};
pub use policy::{Policy, PolicyDecision, PolicyOutcome};
pub use rbac::{Permission, Role, Subject};
pub use task::{Task, TaskPriority, TaskSpec, TaskStatus};
pub use types::{Metadata, ResourceLimits, RetryPolicy, Timestamp};
