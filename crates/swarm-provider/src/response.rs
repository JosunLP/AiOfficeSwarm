//! Normalized response types from provider interactions.

use serde::{Deserialize, Serialize};

use crate::token::TokenUsage;

/// The reason a model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    /// The model finished naturally (end of response).
    Stop,
    /// The model hit the maximum token limit.
    Length,
    /// The model wants to call one or more tools.
    ToolCalls,
    /// The response was filtered by a content filter.
    ContentFilter,
    /// An unknown or vendor-specific finish reason.
    Other(String),
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// A unique identifier for this tool call (used to match results).
    pub id: String,
    /// The name of the tool to invoke.
    pub name: String,
    /// The arguments to pass to the tool (JSON string).
    pub arguments: String,
}

/// A normalized chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// The model that produced this response.
    pub model: String,
    /// The generated text content (may be empty if only tool calls).
    pub content: Option<String>,
    /// Tool calls requested by the model.
    pub tool_calls: Vec<ToolCall>,
    /// Why the model stopped generating.
    pub finish_reason: Option<FinishReason>,
    /// Token usage statistics.
    pub usage: Option<TokenUsage>,
    /// The provider-specific raw response ID.
    pub response_id: Option<String>,
    /// Arbitrary vendor-specific metadata.
    pub extra: serde_json::Value,
}

impl ChatResponse {
    /// Returns the text content, or an empty string if none.
    pub fn text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }

    /// Returns `true` if the model requested tool calls.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// A single embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    /// The index of the input text this embedding corresponds to.
    pub index: usize,
    /// The embedding vector.
    pub values: Vec<f64>,
}

/// A normalized embedding response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    /// The model used.
    pub model: String,
    /// The generated embeddings.
    pub embeddings: Vec<Embedding>,
    /// Token usage statistics.
    pub usage: Option<TokenUsage>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_response_text_fallback() {
        let resp = ChatResponse {
            model: "test".into(),
            content: None,
            tool_calls: vec![],
            finish_reason: Some(FinishReason::Stop),
            usage: None,
            response_id: None,
            extra: serde_json::Value::Null,
        };
        assert_eq!(resp.text(), "");
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn chat_response_with_tool_calls() {
        let resp = ChatResponse {
            model: "test".into(),
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: "{}".into(),
            }],
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
            response_id: None,
            extra: serde_json::Value::Null,
        };
        assert!(resp.has_tool_calls());
    }
}
