//! The primary trait that every AI model provider adapter must implement.

use async_trait::async_trait;

use swarm_core::error::SwarmResult;
use swarm_core::identity::PluginId;

use crate::capabilities::ProviderCapabilities;
use crate::request::{ChatRequest, EmbeddingRequest};
use crate::response::{ChatResponse, EmbeddingResponse};
use crate::streaming::StreamEvent;

/// Health status of a provider.
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    /// Whether the provider is reachable and functional.
    pub healthy: bool,
    /// Optional latency measurement from the health check.
    pub latency_ms: Option<u64>,
    /// Optional human-readable status message.
    pub message: Option<String>,
}

/// The primary trait for AI model provider adapters.
///
/// Implement this trait in a plugin crate to add support for a new provider.
/// The framework never calls vendor SDKs directly — all interaction goes
/// through this trait.
///
/// ## Example (sketch)
///
/// ```rust,ignore
/// struct OpenAiProvider { /* ... */ }
///
/// #[async_trait]
/// impl ModelProvider for OpenAiProvider {
///     fn id(&self) -> PluginId { self.plugin_id }
///     fn name(&self) -> &str { "openai" }
///     fn capabilities(&self) -> ProviderCapabilities { /* ... */ }
///     async fn chat_completion(&self, req: ChatRequest) -> SwarmResult<ChatResponse> { /* ... */ }
///     // ...
/// }
/// ```
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Returns the unique ID of this provider registration.
    fn id(&self) -> PluginId;

    /// A short, stable name for this provider (e.g., `"openai"`, `"anthropic"`).
    fn name(&self) -> &str;

    /// Discover the capabilities of this provider.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Perform a chat completion request.
    async fn chat_completion(&self, request: ChatRequest) -> SwarmResult<ChatResponse>;

    /// Perform a streaming chat completion request.
    ///
    /// The default implementation returns an error indicating streaming is not
    /// supported. Override this if your provider supports streaming.
    async fn chat_completion_stream(&self, _request: ChatRequest) -> SwarmResult<Vec<StreamEvent>> {
        Err(swarm_core::error::SwarmError::Internal {
            reason: format!("Provider '{}' does not support streaming", self.name()),
        })
    }

    /// Generate embeddings for the given inputs.
    ///
    /// The default implementation returns an error indicating embeddings are
    /// not supported. Override this if your provider supports embeddings.
    async fn embedding(&self, _request: EmbeddingRequest) -> SwarmResult<EmbeddingResponse> {
        Err(swarm_core::error::SwarmError::Internal {
            reason: format!("Provider '{}' does not support embeddings", self.name()),
        })
    }

    /// Perform a health check against the provider endpoint.
    async fn health_check(&self) -> SwarmResult<ProviderHealth>;
}
