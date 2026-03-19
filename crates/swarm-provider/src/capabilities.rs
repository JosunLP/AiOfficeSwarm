//! Provider capability discovery and declaration.
//!
//! Every provider declares its capabilities via [`ProviderCapabilities`].
//! The framework uses this to route requests, negotiate features, and
//! provide clear errors when a capability is unavailable.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A descriptor for a specific model offered by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// The model identifier as recognized by the provider (e.g., `"gpt-4o"`).
    pub model_id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Maximum context window in tokens.
    pub max_context_tokens: Option<u64>,
    /// Maximum output tokens.
    pub max_output_tokens: Option<u64>,
    /// Whether this model supports tool/function calling.
    pub supports_tools: bool,
    /// Whether this model supports vision/image inputs.
    pub supports_vision: bool,
    /// Whether this model supports streaming responses.
    pub supports_streaming: bool,
    /// Whether this model supports structured JSON output mode.
    pub supports_json_mode: bool,
    /// Whether this model is a reasoning/chain-of-thought model.
    pub is_reasoning_model: bool,
}

/// The complete capability declaration of an AI model provider.
///
/// Providers populate this at registration time. The framework uses it for
/// routing, feature negotiation, and graceful degradation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Whether the provider supports chat completion.
    pub chat_completion: bool,
    /// Whether the provider supports streaming responses.
    pub streaming: bool,
    /// Whether the provider supports tool/function calling.
    pub tool_calling: bool,
    /// Whether the provider supports reasoning/chain-of-thought models.
    pub reasoning: bool,
    /// Whether the provider supports embedding generation.
    pub embeddings: bool,
    /// Whether the provider supports speech-to-text or text-to-speech.
    pub speech: bool,
    /// Whether the provider supports multimodal inputs (images, audio, etc.).
    pub multimodal: bool,
    /// Whether the provider supports vision/image understanding.
    pub vision: bool,
    /// Whether the provider supports structured JSON output mode.
    pub json_mode: bool,
    /// Models available from this provider.
    pub models: Vec<ModelDescriptor>,
    /// Arbitrary vendor-specific capabilities.
    pub custom: HashMap<String, serde_json::Value>,
}

impl ProviderCapabilities {
    /// Check whether this provider satisfies the given requirement.
    pub fn satisfies(&self, required: &ProviderCapabilities) -> bool {
        let checks = [
            (!required.chat_completion || self.chat_completion),
            (!required.streaming || self.streaming),
            (!required.tool_calling || self.tool_calling),
            (!required.reasoning || self.reasoning),
            (!required.embeddings || self.embeddings),
            (!required.speech || self.speech),
            (!required.multimodal || self.multimodal),
            (!required.vision || self.vision),
            (!required.json_mode || self.json_mode),
        ];
        checks.iter().all(|&c| c)
    }

    /// Return a list of capability names that are required but not satisfied.
    pub fn missing_capabilities(&self, required: &ProviderCapabilities) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if required.chat_completion && !self.chat_completion {
            missing.push("chat_completion");
        }
        if required.streaming && !self.streaming {
            missing.push("streaming");
        }
        if required.tool_calling && !self.tool_calling {
            missing.push("tool_calling");
        }
        if required.reasoning && !self.reasoning {
            missing.push("reasoning");
        }
        if required.embeddings && !self.embeddings {
            missing.push("embeddings");
        }
        if required.speech && !self.speech {
            missing.push("speech");
        }
        if required.multimodal && !self.multimodal {
            missing.push("multimodal");
        }
        if required.vision && !self.vision {
            missing.push("vision");
        }
        if required.json_mode && !self.json_mode {
            missing.push("json_mode");
        }
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_satisfy_subset() {
        let provider = ProviderCapabilities {
            chat_completion: true,
            streaming: true,
            tool_calling: true,
            ..Default::default()
        };
        let required = ProviderCapabilities {
            chat_completion: true,
            ..Default::default()
        };
        assert!(provider.satisfies(&required));
    }

    #[test]
    fn capabilities_fail_when_missing() {
        let provider = ProviderCapabilities {
            chat_completion: true,
            ..Default::default()
        };
        let required = ProviderCapabilities {
            chat_completion: true,
            embeddings: true,
            ..Default::default()
        };
        assert!(!provider.satisfies(&required));
        assert_eq!(provider.missing_capabilities(&required), vec!["embeddings"]);
    }
}
