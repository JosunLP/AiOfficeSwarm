//! # swarm-provider
//!
//! Provider-agnostic AI model integration layer for the AiOfficeSwarm framework.
//!
//! This crate defines the abstractions for integrating with any AI model
//! provider (OpenAI, Anthropic, Ollama, Azure, local models, etc.) without
//! coupling the core domain to any specific vendor.
//!
//! ## Key abstractions
//!
//! - [`ModelProvider`] — the primary trait every provider adapter implements.
//! - [`ProviderCapabilities`] — declares what a provider can do (chat, tools,
//!   embeddings, streaming, etc.).
//! - [`ChatRequest`] / [`ChatResponse`] — normalized request/response types.
//! - [`StreamEvent`] — normalized streaming delta events.
//! - [`EmbeddingRequest`] / [`EmbeddingResponse`] — normalized embedding I/O.
//! - [`ProviderRegistry`] — runtime registry of available providers.
//! - [`ProviderRouter`] — strategy-based provider selection and failover.
//! - [`TokenUsage`] — normalized token and cost accounting.
//!
//! ## Design principles
//!
//! - **No vendor lock-in**: the core never imports a provider SDK directly.
//! - **Capability discovery**: providers declare their features; callers negotiate.
//! - **Graceful degradation**: missing capabilities produce clear errors or fallbacks.
//! - **Streaming first**: streaming is a first-class response mode.
//! - **Cost awareness**: every response carries token usage and optional cost data.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod capabilities;
pub mod registry;
pub mod request;
pub mod response;
pub mod router;
pub mod streaming;
pub mod token;
pub mod traits;

// Re-exports for ergonomic use.
pub use capabilities::ProviderCapabilities;
pub use registry::ProviderRegistry;
pub use request::{ChatMessage, ChatRequest, EmbeddingRequest, MessageRole};
pub use response::{ChatResponse, EmbeddingResponse, FinishReason, ToolCall};
pub use router::{
    CostPreference, LatencyPreference, ProviderRouter, RoutingContext, RoutingStrategy,
};
pub use streaming::StreamEvent;
pub use token::TokenUsage;
pub use traits::ModelProvider;
