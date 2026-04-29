use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::common::{PromptOption, Reaction};

// ---- Core ----

/// User message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessageEvent {
    pub text: String,
}

/// Final assistant response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageEvent {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Streaming assistant output chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantStreamEvent {
    pub phase: String,
    pub content: String,
    pub index: u64,
}

// ---- Tools ----

/// Tool invocation started.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub call_id: String,
    pub tool: String,
    pub arguments: String,
}

/// Tool execution approved by user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovedEvent {
    pub tool: String,
    pub decision: String,
}

/// Progress update from a running tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressEvent {
    pub call_id: String,
    pub tool: String,
    pub message: String,
}

/// Tool execution completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    pub call_id: String,
    pub tool: String,
    pub result: String,
    pub is_error: bool,
}

// ---- Prompts ----

/// Prompt request sent to the user for approval or input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequestEvent {
    pub prompt_type: String,
    pub label: String,
    pub token: String,
    #[serde(default)]
    pub options: Vec<PromptOption>,
}

/// User response to a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponseEvent {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ---- LLM internals ----

/// LLM call metrics for cost tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallEvent {
    pub phase: String,
    pub tokens_input: u64,
    pub tokens_output: u64,
}

// ---- Conversations ----

/// Conversation created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationCreatedEvent {
    pub conversation_id: String,
}

/// Conversation history cleared.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationClearEvent {
    pub conversation_id: String,
    pub connector: String,
    pub channel_id: String,
}

// ---- Flow control ----

/// ReactLoop cancelled by user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCancelledEvent {
    pub reason: String,
}

/// ReactLoop yielded by LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentYieldedEvent {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_on: Option<String>,
}

// ---- Connectors ----

/// Incoming message from a connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorMessageEvent {
    pub connector: String,
    pub channel_id: String,
    pub message_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

/// Outbound reply to a connector channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorReplyEvent {
    pub connector: String,
    pub channel_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(default)]
    pub feedback: bool,
}

/// Typing indicator for a connector channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorTypingEvent {
    pub connector: String,
    pub channel_id: String,
}

/// Add a reaction to a connector message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorAddReactionEvent {
    pub connector: String,
    pub channel_id: String,
    pub message_id: String,
    pub reaction: Reaction,
}

/// Clear reactions from a connector message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorClearReactionsEvent {
    pub connector: String,
    pub channel_id: String,
    pub message_id: String,
}

// ---- Scheduler ----

/// Schedule triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleTriggerEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
}

/// Schedule entry created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleCreatedEvent {
    pub entry_id: String,
    pub title: String,
    pub source: String,
}

/// Schedule entry removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRemovedEvent {
    pub entry_id: String,
    pub title: String,
}

// ---- Pairing ----

/// Pairing request from a device or chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PairingRequestEvent {
    Device {
        request_id: String,
        client_type: Option<String>,
        label: Option<String>,
    },
    Chat {
        request_id: String,
        platform: Option<String>,
        server_id: Option<String>,
        channel_id: Option<String>,
        user_id: Option<String>,
        display_name: Option<String>,
    },
}

/// Pairing approved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PairingApprovedEvent {
    Device {
        request_id: String,
        approved_by: String,
        device_id: Option<String>,
    },
    Chat {
        request_id: String,
        approved_by: String,
        policy_name: Option<String>,
    },
}

/// Pairing rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRejectedEvent {
    pub request_id: String,
    pub rejected_by: String,
}

// ---- Error ----

/// Generic error event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
