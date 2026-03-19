//! # swarm-memory
//!
//! First-class memory subsystem for AI agents in the AiOfficeSwarm framework.
//!
//! This crate provides the abstractions and core types for agent memory,
//! supporting multiple scopes (session, task, agent, team, tenant, long-term)
//! and multiple types (structured, semantic, episodic, procedural, knowledge
//! references, summaries).
//!
//! ## Design principles
//!
//! - **Privacy-aware**: memory access is governed by policy and scope rules.
//! - **Backend-agnostic**: the [`MemoryBackend`] trait can be implemented for
//!   any storage engine (in-memory, PostgreSQL, Redis, vector DBs, etc.).
//! - **Auditable**: sensitive memory operations produce audit events.
//! - **Retention-managed**: expiration, summarization, and redaction are
//!   first-class concerns.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod access;
pub mod backend;
pub mod entry;
pub mod in_memory;
pub mod query;
pub mod retention;

pub use access::MemoryAccessProfile;
pub use backend::MemoryBackend;
pub use entry::{MemoryEntry, MemoryId, MemoryScope, MemoryType};
pub use query::MemoryQuery;
pub use retention::RetentionPolicy;
