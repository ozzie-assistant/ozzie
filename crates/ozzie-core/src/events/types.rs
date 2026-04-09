use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

// Re-export shared types from ozzie-types
pub use ozzie_types::{EventKind, PromptOption, ToolConstraint};

// Reaction is re-exported from crate::connector (which adds the ALL const)
use crate::connector::Reaction;

/// Identifies the component that emitted an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSource {
    Agent,
    Hub,
    Ws,
    Plugin,
    Skill,
    Scheduler,
    Mcp,
    Connector,
}

/// Typed event payload — each variant carries exactly its fields.
///
/// Payloads are composed from `ozzie-types` structs where possible.
/// Variants that have no fields in ozzie-types use inline fields for simplicity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    // User -> Agent
    #[serde(rename = "user.message")]
    UserMessage {
        text: String,
        /// Image blob references attached to the message.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ozzie_types::BlobRef>,
    },

    // Agent -> Client
    #[serde(rename = "assistant.message")]
    AssistantMessage {
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename = "assistant.stream")]
    AssistantStream {
        phase: String,
        content: String,
        index: u64,
    },

    // Tool calls
    #[serde(rename = "tool.call")]
    ToolCall {
        call_id: String,
        tool: String,
        arguments: String,
    },
    #[serde(rename = "tool.approved")]
    ToolApproved { tool: String, decision: String },
    #[serde(rename = "tool.progress")]
    ToolProgress {
        call_id: String,
        tool: String,
        message: String,
    },
    #[serde(rename = "tool.result")]
    ToolResult {
        call_id: String,
        tool: String,
        result: String,
        is_error: bool,
    },

    // Prompts (approval flow)
    #[serde(rename = "prompt.request")]
    PromptRequest {
        prompt_type: String,
        label: String,
        token: String,
        options: Vec<PromptOption>,
    },
    #[serde(rename = "prompt.response")]
    PromptResponse {
        token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(flatten)]
        extra: std::collections::HashMap<String, serde_json::Value>,
    },

    // Internal (analytics/tracing)
    #[serde(rename = "internal.llm.call")]
    LlmCall {
        phase: String,
        tokens_input: u64,
        tokens_output: u64,
    },

    // Session lifecycle
    #[serde(rename = "session.created")]
    SessionCreated { session_id: String },
    #[serde(rename = "session.closed")]
    SessionClosed { session_id: String },

    // Connectors
    #[serde(rename = "incoming.message")]
    IncomingMessage,
    #[serde(rename = "outgoing.message")]
    OutgoingMessage,
    #[serde(rename = "connector.message")]
    ConnectorMessage {
        connector: String,
        channel_id: String,
        message_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        roles: Vec<String>,
    },
    #[serde(rename = "connector.reply")]
    ConnectorReply {
        connector: String,
        channel_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to_id: Option<String>,
        #[serde(default)]
        feedback: bool,
    },
    #[serde(rename = "connector.typing")]
    ConnectorTyping {
        connector: String,
        channel_id: String,
    },
    #[serde(rename = "connector.add_reaction")]
    ConnectorAddReaction {
        connector: String,
        channel_id: String,
        message_id: String,
        reaction: Reaction,
    },
    #[serde(rename = "connector.clear_reactions")]
    ConnectorClearReactions {
        connector: String,
        channel_id: String,
        message_id: String,
    },
    #[serde(rename = "session.clear")]
    SessionClear {
        session_id: String,
        connector: String,
        channel_id: String,
    },

    // Flow control
    #[serde(rename = "agent.cancelled")]
    AgentCancelled { reason: String },
    #[serde(rename = "agent.yielded")]
    AgentYielded {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume_on: Option<String>,
    },

    // Scheduler
    #[serde(rename = "schedule.trigger")]
    ScheduleTrigger {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry_id: Option<String>,
    },
    #[serde(rename = "schedule.created")]
    ScheduleCreated {
        entry_id: String,
        title: String,
        source: String,
    },
    #[serde(rename = "schedule.removed")]
    ScheduleRemoved { entry_id: String, title: String },

    // Skills
    #[serde(rename = "skill.started")]
    SkillStarted,
    #[serde(rename = "skill.completed")]
    SkillCompleted,
    #[serde(rename = "skill.step.started")]
    SkillStepStarted,
    #[serde(rename = "skill.step.completed")]
    SkillStepCompleted,

    // Context
    #[serde(rename = "context.layered")]
    ContextLayered,

    // Dream consolidation
    #[serde(rename = "dream.completed")]
    DreamCompleted {
        sessions_processed: usize,
        sessions_errored: usize,
        profile_entries_added: usize,
        memories_created: usize,
    },

    // Pairing — device
    #[serde(rename = "pairing.request.device")]
    PairingRequestDevice {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    // Pairing — chat
    #[serde(rename = "pairing.request.chat")]
    PairingRequestChat {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },
    #[serde(rename = "pairing.approved.device")]
    PairingApprovedDevice {
        request_id: String,
        approved_by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    },
    #[serde(rename = "pairing.approved.chat")]
    PairingApprovedChat {
        request_id: String,
        approved_by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        policy_name: Option<String>,
    },
    #[serde(rename = "pairing.rejected")]
    PairingRejected {
        request_id: String,
        rejected_by: String,
    },

    // Error
    #[serde(rename = "error")]
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl EventPayload {
    /// Returns the `EventKind` for this payload variant.
    pub fn event_kind(&self) -> EventKind {
        match self {
            Self::UserMessage { .. } => EventKind::UserMessage,
            Self::AssistantMessage { .. } => EventKind::AssistantMessage,
            Self::AssistantStream { .. } => EventKind::AssistantStream,
            Self::ToolCall { .. } => EventKind::ToolCall,
            Self::ToolApproved { .. } => EventKind::ToolApproved,
            Self::ToolProgress { .. } => EventKind::ToolProgress,
            Self::ToolResult { .. } => EventKind::ToolResult,
            Self::PromptRequest { .. } => EventKind::PromptRequest,
            Self::PromptResponse { .. } => EventKind::PromptResponse,
            Self::LlmCall { .. } => EventKind::LlmCall,
            Self::SessionCreated { .. } => EventKind::SessionCreated,
            Self::SessionClosed { .. } => EventKind::SessionClosed,
            Self::IncomingMessage => EventKind::IncomingMessage,
            Self::OutgoingMessage => EventKind::OutgoingMessage,
            Self::ConnectorMessage { .. } => EventKind::ConnectorMessage,
            Self::ConnectorReply { .. } => EventKind::ConnectorReply,
            Self::ConnectorTyping { .. } => EventKind::ConnectorTyping,
            Self::ConnectorAddReaction { .. } => EventKind::ConnectorAddReaction,
            Self::ConnectorClearReactions { .. } => EventKind::ConnectorClearReactions,
            Self::SessionClear { .. } => EventKind::SessionClear,
            Self::AgentCancelled { .. } => EventKind::AgentCancelled,
            Self::AgentYielded { .. } => EventKind::AgentYielded,
            Self::ScheduleTrigger { .. } => EventKind::ScheduleTrigger,
            Self::ScheduleCreated { .. } => EventKind::ScheduleCreated,
            Self::ScheduleRemoved { .. } => EventKind::ScheduleRemoved,
            Self::SkillStarted => EventKind::SkillStarted,
            Self::SkillCompleted => EventKind::SkillCompleted,
            Self::SkillStepStarted => EventKind::SkillStepStarted,
            Self::SkillStepCompleted => EventKind::SkillStepCompleted,
            Self::ContextLayered => EventKind::ContextLayered,
            Self::DreamCompleted { .. } => EventKind::DreamCompleted,
            Self::PairingRequestDevice { .. } => EventKind::PairingRequestDevice,
            Self::PairingRequestChat { .. } => EventKind::PairingRequestChat,
            Self::PairingApprovedDevice { .. } => EventKind::PairingApprovedDevice,
            Self::PairingApprovedChat { .. } => EventKind::PairingApprovedChat,
            Self::PairingRejected { .. } => EventKind::PairingRejected,
            Self::Error { .. } => EventKind::Error,
        }
    }
}

/// An event in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub source: EventSource,
    #[serde(flatten)]
    pub payload: EventPayload,
}

static EVENT_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_event_id() -> String {
    let seq = EVENT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{now}-{seq}")
}

impl EventPayload {
    /// Creates a text-only `UserMessage` (no image attachments).
    pub fn user_message(text: impl Into<String>) -> Self {
        Self::UserMessage {
            text: text.into(),
            images: Vec::new(),
        }
    }

    /// Creates a `UserMessage` with image attachments.
    pub fn user_message_with_images(text: impl Into<String>, images: Vec<ozzie_types::BlobRef>) -> Self {
        Self::UserMessage {
            text: text.into(),
            images,
        }
    }
}

impl Event {
    /// Creates a new event with the current timestamp.
    pub fn new(source: EventSource, payload: EventPayload) -> Self {
        Self {
            id: generate_event_id(),
            session_id: None,
            timestamp: Utc::now(),
            source,
            payload,
        }
    }

    /// Creates a new event with session context.
    pub fn with_session(
        source: EventSource,
        payload: EventPayload,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            id: generate_event_id(),
            session_id: Some(session_id.into()),
            timestamp: Utc::now(),
            source,
            payload,
        }
    }

    /// Returns the event type string, derived from `EventKind`.
    pub fn event_type(&self) -> &'static str {
        self.payload.event_kind().as_str()
    }

    /// Returns true if this event matches the given type string.
    pub fn is_type(&self, type_str: &str) -> bool {
        self.event_type() == type_str
    }
}
