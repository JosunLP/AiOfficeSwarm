//! # swarm-policy
//!
//! Policy engine for the AiOfficeSwarm framework.
//!
//! This crate provides:
//!
//! - [`PolicyEngine`]: Evaluates a prioritized list of [`Policy`] trait objects
//!   and returns an aggregated [`PolicyDecision`].
//! - [`RbacEngine`]: Enforces Role-Based Access Control using the [`Role`] and
//!   [`Permission`] primitives from `swarm-core`.
//! - Built-in policies: [`DenyAllPolicy`], [`AllowAllPolicy`], and
//!   [`ActionAllowlistPolicy`].
//!
//! ## Integration
//! Embedding applications can invoke the policy engine around sensitive
//! operations (task scheduling, plugin invocation, agent creation, etc.).
//! The current `swarm-orchestrator` crate does not wire these checks in
//! automatically.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod engine;
pub mod rbac_engine;
pub mod builtin;

pub use engine::PolicyEngine;
pub use rbac_engine::RbacEngine;
pub use builtin::{AllowAllPolicy, DenyAllPolicy, ActionAllowlistPolicy};
