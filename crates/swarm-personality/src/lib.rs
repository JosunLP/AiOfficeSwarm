//! # swarm-personality
//!
//! First-class personality system for AI agents in the AiOfficeSwarm framework.
//!
//! Personalities influence an agent's communication style, decision-making
//! tendencies, risk tolerance, collaboration patterns, escalation behavior,
//! domain specialization, and response formatting.
//!
//! ## Design principles
//!
//! - **Configurable**: personalities are data, not code.
//! - **Versioned**: every personality profile carries a version string.
//! - **Composable**: personalities can be layered (base → org → role → task).
//! - **Policy-constrained**: boundaries prevent personalities from bypassing
//!   security, policy, or authorization rules.
//! - **Extensible**: custom traits can be added via `custom_traits`.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod boundary;
pub mod profile;
pub mod registry;
pub mod traits;

pub use boundary::PersonalityBoundary;
pub use profile::{
    CollaborationPattern, CommunicationStyle, DecisionTendencies, EscalationBehavior,
    PersonalityProfile, ResponseFormatting, RiskTolerance,
};
pub use registry::PersonalityRegistry;
pub use traits::PersonalityId;
