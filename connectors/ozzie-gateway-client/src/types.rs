// Re-export canonical types from ozzie-types for backward compatibility.
pub use ozzie_types::events::{
    AgentCancelledEvent, AgentYieldedEvent, AssistantMessageEvent, AssistantStreamEvent,
    ConnectorReplyEvent, ErrorEvent, PromptRequestEvent, ToolCallEvent, ToolProgressEvent,
    ToolResultEvent,
};
pub use ozzie_types::common::PromptOption;
pub use ozzie_types::requests::{
    ArchivedResult, CloseConversationParams, ConversationResult as SessionInfo,
    ConversationSummaryDto, ConversationsListResult, ListConversationsParams,
    NewConversationParams, OpenConversationParams as OpenConversationOpts, PromptResponseParams,
    SendConnectorMessageParams as ConnectorMessageParams, SwitchConversationParams,
    SwitchedResult,
};

// ---- Notifications ----

/// Typed gateway notification.
///
/// Each variant wraps the canonical event type from `ozzie-types`.
#[derive(Debug, Clone)]
pub enum Notification {
    AssistantStream(AssistantStreamEvent),
    AssistantMessage(AssistantMessageEvent),
    ToolCall(ToolCallEvent),
    ToolResult(ToolResultEvent),
    ToolProgress(ToolProgressEvent),
    PromptRequest(PromptRequestEvent),
    AgentCancelled(AgentCancelledEvent),
    AgentYielded(AgentYieldedEvent),
    ConnectorReply(ConnectorReplyEvent),
    Error(ErrorEvent),
    Unknown {
        method: String,
        params: serde_json::Value,
    },
}
