//! Normalized streaming event types.
//!
//! When a provider supports streaming, responses arrive as a series of
//! [`StreamEvent`] values. Provider adapters translate vendor-specific
//! server-sent events or WebSocket frames into this normalized format.

use serde::{Deserialize, Serialize};

use crate::response::ToolCall;
use crate::token::TokenUsage;

/// A single event in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// A chunk of text content.
    ContentDelta {
        /// The incremental text fragment.
        delta: String,
    },
    /// A partial tool call being assembled.
    ToolCallDelta {
        /// Index of the tool call being constructed.
        index: usize,
        /// The tool call ID (may be set only in the first delta).
        id: Option<String>,
        /// The tool name (may be set only in the first delta).
        name: Option<String>,
        /// Incremental arguments fragment.
        arguments_delta: Option<String>,
    },
    /// The stream has completed for this response.
    Done {
        /// Why the model stopped generating.
        finish_reason: Option<String>,
        /// Final token usage (if the provider reports it at stream end).
        usage: Option<TokenUsage>,
    },
    /// An error occurred during streaming.
    Error {
        /// Error description.
        message: String,
    },
}

/// Utility to assemble streaming tool calls from deltas.
#[derive(Debug, Default)]
pub struct ToolCallAssembler {
    pending: Vec<PartialToolCall>,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAssembler {
    /// Create a new assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a tool call delta into the assembler.
    pub fn push_delta(
        &mut self,
        index: usize,
        id: Option<&str>,
        name: Option<&str>,
        arguments_delta: Option<&str>,
    ) {
        // Grow the list if needed.
        while self.pending.len() <= index {
            self.pending.push(PartialToolCall::default());
        }
        let entry = &mut self.pending[index];
        if let Some(id) = id {
            entry.id = id.to_string();
        }
        if let Some(name) = name {
            entry.name = name.to_string();
        }
        if let Some(args) = arguments_delta {
            entry.arguments.push_str(args);
        }
    }

    /// Finalize and return the assembled tool calls.
    pub fn finish(self) -> Vec<ToolCall> {
        self.pending
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.name,
                arguments: tc.arguments,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_assembler_basic() {
        let mut asm = ToolCallAssembler::new();
        asm.push_delta(0, Some("call_1"), Some("search"), Some("{\"q\":"));
        asm.push_delta(0, None, None, Some("\"hello\"}"));

        let calls = asm.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[0].arguments, "{\"q\":\"hello\"}");
    }
}
