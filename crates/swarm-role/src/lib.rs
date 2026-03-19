//! # swarm-role
//!
//! First-class role management subsystem for the AiOfficeSwarm framework.
//!
//! This crate transforms organizational role definitions (such as those in
//! the `roles/` directory) into typed, governable, versionable, and executable
//! runtime constructs. Roles are not cosmetic labels — they influence
//! orchestration, tool access, memory boundaries, learning policies,
//! personality overlays, supervision hierarchies, and compliance controls.
//!
//! ## Architecture
//!
//! The role subsystem distinguishes three layers:
//!
//! | Layer | Type | Purpose |
//! |-------|------|---------|
//! | **Source** | [`RawRoleSource`] | Unvalidated parse result from a role file |
//! | **Specification** | [`RoleSpec`] | Normalized, validated, typed role definition |
//! | **Runtime profile** | [`EffectiveRoleProfile`] | Policy-resolved, tenant-overridden runtime state |
//!
//! ## Subsystem components
//!
//! - [`RoleId`] — unique identifier for a role.
//! - [`RoleSpec`] — the canonical typed specification.
//! - [`RoleLoader`] — loads role files from a directory on disk.
//! - [`RoleParser`] — parses Markdown role files into [`RawRoleSource`].
//! - [`RoleNormalizer`] — converts raw sources into validated [`RoleSpec`]s.
//! - [`RoleValidator`] — validates role specifications against structural rules.
//! - [`RoleRegistry`] — concurrent registry of loaded role specifications.
//! - [`RoleResolver`] — resolves effective runtime profiles with policy/tenant overrides.
//! - [`RoleHierarchy`] — models the organigram and supervision relationships.
//!
//! ## Conflict resolution
//!
//! When role definitions conflict with policies, permissions, memory rules,
//! learning rules, or platform-wide security controls, the **more restrictive
//! rule wins** by default unless an explicit authorized override exists.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod error;
pub mod hierarchy;
pub mod loader;
pub mod model;
pub mod normalizer;
pub mod parser;
pub mod policy_binding;
pub mod registry;
pub mod resolver;
pub mod validator;

// Feature-gated bridges to sibling crates.
#[cfg(feature = "learning")]
pub mod learning_bridge;
#[cfg(feature = "memory")]
pub mod memory_bridge;
#[cfg(feature = "personality")]
pub mod personality_bridge;

// Re-export primary types at the crate root.
pub use error::{RoleError, RoleResult};
pub use hierarchy::RoleHierarchy;
pub use loader::RoleLoader;
pub use model::{
    DepartmentCategory, EffectiveRoleProfile, EscalationPolicy, RawRoleSource,
    RoleCollaborationRule, RoleId, RoleLearningPolicy, RoleMemoryPolicy, RoleMetadata, RoleSpec,
    RoleToolPolicy, TenantRoleOverride,
};
pub use normalizer::RoleNormalizer;
pub use parser::RoleParser;
pub use policy_binding::RolePolicyBinding;
pub use registry::RoleRegistry;
pub use resolver::RoleResolver;
pub use validator::{RoleValidator, ValidationIssue, ValidationSeverity};
