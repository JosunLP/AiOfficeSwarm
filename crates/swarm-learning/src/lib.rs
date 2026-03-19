//! # swarm-learning
//!
//! First-class learning subsystem for AI agents in the AiOfficeSwarm framework.
//!
//! Learning in AiOfficeSwarm is **controlled, auditable, and safe for enterprise
//! environments**. It is not vague self-modification — every learning mechanism
//! is explicit, bounded, reviewable, and disableable.
//!
//! ## Learning mechanisms
//!
//! | Mechanism | Description |
//! |-----------|-------------|
//! | Preference adaptation | Adjust weights/rankings based on feedback |
//! | Pattern extraction | Identify successful task sequences |
//! | Feedback incorporation | Quality signals from humans or automated checks |
//! | Plan templates | Reusable workflow patterns |
//! | Scoring improvements | Update heuristic scores for routing |
//! | Knowledge accumulation | Org-specific facts from task outcomes |
//! | Configuration evolution | Suggest config changes (requires approval) |
//! | Fine-tuning hooks | Export training data (requires approval) |
//!
//! ## Design principles
//!
//! - **Reviewable**: all learning deltas store full context.
//! - **Auditable**: every approve/reject/rollback is logged.
//! - **Rollback**: applied learning can be reverted.
//! - **Permission-gated**: requires `learn:*` RBAC permissions.
//! - **Tenant-isolated**: learning data never crosses tenant boundaries.
//! - **Disableable**: per tenant, team, agent, or workflow via policy.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod event;
pub mod output;
pub mod scope;
pub mod store;
pub mod strategy;

pub use event::LearningEvent;
pub use output::{LearningOutput, LearningResult, LearningStatus};
pub use scope::LearningScope;
pub use store::LearningStore;
pub use strategy::LearningStrategy;
