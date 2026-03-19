//! Normalized request types for provider interactions.
//!
//! These types abstract away vendor-specific request formats. Provider adapters
//! translate from these normalized types to their vendor-specific wire format.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The role of a message participant in a chat conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    /// System/instruction message.
    System,
    /// User/human message.
    User,
    /// Assistant/model response.
    Assistant,
    /// Tool/function result message.
    Tool,
}

/// The content of a message — either plain text or multimodal parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text content.
    Text(String),
    /// Multimodal content parts (text, images, audio, etc.).
    Parts(Vec<ContentPart>),
}

/// A single part of a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentPart {
    /// A text segment.
    Text {
        /// The text content.
        text: String,
    },
    /// An image provided as a URL or base64-encoded data.
    Image {
        /// The image source: URL or base64 data URI.
        source: String,
        /// Optional media type (e.g., `"image/png"`).
        media_type: Option<String>,
    },
    /// An audio segment provided as base64-encoded data.
    Audio {
        /// Base64-encoded audio data.
        data: String,
        /// Audio format (e.g., `"wav"`, `"mp3"`).
        format: String,
    },
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role of the message author.
    pub role: MessageRole,
    /// The message content.
    pub content: MessageContent,
    /// Optional name for the message author (used for multi-agent conversations).
    pub name: Option<String>,
    /// Tool call ID this message is responding to (for `Tool` role messages).
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// Create a simple text message.
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: MessageContent::Text(content.into()),
            name: None,
            tool_call_id: None,
        }
    }

    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(MessageRole::System, content)
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(MessageRole::User, content)
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(MessageRole::Assistant, content)
    }
}

/// A tool/function declaration that the model may call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// The tool name (must be unique within a request).
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the expected parameters.
    pub parameters_schema: serde_json::Value,
}

/// How the model should handle tool usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ToolChoice {
    /// The model decides whether to use tools.
    #[default]
    Auto,
    /// The model must not use tools.
    None,
    /// The model must use at least one tool.
    Required,
    /// The model must call a specific tool.
    Specific {
        /// The name of the required tool.
        name: String,
    },
}

/// A normalized chat completion request.
///
/// Provider adapters translate this into their vendor-specific format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// The model identifier to use (e.g., `"gpt-4o"`, `"claude-3-opus"`).
    pub model: String,
    /// The conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Optional tools the model may call.
    pub tools: Vec<ToolDefinition>,
    /// How the model should handle tool usage.
    pub tool_choice: ToolChoice,
    /// Maximum number of tokens to generate.
    pub max_tokens: Option<u64>,
    /// Sampling temperature (0.0 = deterministic, 2.0 = creative).
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    pub top_p: Option<f64>,
    /// Stop sequences that terminate generation.
    pub stop: Vec<String>,
    /// Whether to request streaming response.
    pub stream: bool,
    /// Whether to request structured JSON output.
    pub json_mode: bool,
    /// Optional timeout for this request.
    pub timeout: Option<Duration>,
    /// Arbitrary vendor-specific parameters.
    pub extra: HashMap<String, serde_json::Value>,
}

impl ChatRequest {
    /// Create a minimal chat request with a single user message.
    pub fn simple(model: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![ChatMessage::user(message)],
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: Vec::new(),
            stream: false,
            json_mode: false,
            timeout: None,
            extra: HashMap::new(),
        }
    }
}

/// A normalized embedding request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    /// The model to use for embedding.
    pub model: String,
    /// The texts to embed.
    pub inputs: Vec<String>,
    /// Optional encoding format hint.
    pub encoding_format: Option<String>,
    /// Optional dimensionality hint (for models that support it).
    pub dimensions: Option<u64>,
}

impl EmbeddingRequest {
    /// Create a request to embed a single text.
    pub fn single(model: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            inputs: vec![text.into()],
            encoding_format: None,
            dimensions: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_chat_request() {
        let req = ChatRequest::simple("gpt-4o", "Hello");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert!(!req.stream);
    }

    #[test]
    fn chat_message_constructors() {
        let sys = ChatMessage::system("You are helpful.");
        assert_eq!(sys.role, MessageRole::System);

        let usr = ChatMessage::user("Hi");
        assert_eq!(usr.role, MessageRole::User);

        let asst = ChatMessage::assistant("Hello!");
        assert_eq!(asst.role, MessageRole::Assistant);
    }
}
