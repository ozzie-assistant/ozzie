mod auth;
mod errors;
mod fallback;
pub(crate) mod helpers;
pub mod providers;
mod resilience;
pub mod schema;

pub use auth::*;
pub use errors::*;
pub use fallback::FallbackProvider;
pub use resilience::*;

use std::pin::Pin;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

/// Chat message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for ChatRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

/// A chat message for LLM interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Why the model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of response.
    Stop,
    /// Model requested tool execution.
    ToolUse,
    /// Hit the maximum output token limit.
    MaxTokens,
    /// Content filtered by safety.
    Safety,
    /// Provider-specific reason.
    Other(String),
}

/// A complete chat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub usage: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// The model that actually produced this response (useful for fallback chains).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A streaming delta.
#[derive(Debug, Clone)]
pub enum ChatDelta {
    /// Text content chunk.
    Content(String),
    /// Reasoning/thinking content chunk (extended thinking, DeepSeek, o1/o3).
    Reasoning(String),
    /// Tool call chunk.
    ToolCallStart { id: String, name: String },
    /// Tool call arguments chunk.
    ToolCallDelta { id: String, arguments: String },
    /// Stream finished.
    Done {
        usage: TokenUsage,
        stop_reason: Option<StopReason>,
        model: Option<String>,
    },
}

/// Token usage for a single response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Tokens read from prompt cache (Anthropic, OpenAI).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_read_tokens: u64,
    /// Tokens written to prompt cache (Anthropic).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_tokens: u64,
    /// Internal reasoning tokens (OpenAI o1/o3).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reasoning_tokens: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// Tool definition for the model.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool parameters.
    pub parameters: schemars::schema::RootSchema,
}

/// LLM Provider trait — the core abstraction.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Sends a chat request and returns a complete response.
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError>;

    /// Sends a chat request and returns a streaming response.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError>;

    /// Returns the provider's name (e.g., "anthropic", "ollama").
    fn name(&self) -> &str;
}
