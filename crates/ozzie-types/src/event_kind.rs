use serde::{Deserialize, Serialize};

/// Well-known event type identifiers — single source of truth.
///
/// Wire strings are defined once here via `as_str()`. Both `EventPayload`
/// (ozzie-core) and `Frame` (ozzie-protocol) derive from this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    // Core
    #[serde(rename = "user.message")]
    UserMessage,
    #[serde(rename = "assistant.message")]
    AssistantMessage,
    #[serde(rename = "assistant.stream")]
    AssistantStream,
    // Tools
    #[serde(rename = "tool.call")]
    ToolCall,
    #[serde(rename = "tool.approved")]
    ToolApproved,
    #[serde(rename = "tool.progress")]
    ToolProgress,
    #[serde(rename = "tool.result")]
    ToolResult,
    // Prompts
    #[serde(rename = "prompt.request")]
    PromptRequest,
    #[serde(rename = "prompt.response")]
    PromptResponse,
    // Sessions
    #[serde(rename = "session.created")]
    SessionCreated,
    #[serde(rename = "session.closed")]
    SessionClosed,
    #[serde(rename = "session.clear")]
    SessionClear,
    // LLM internals
    #[serde(rename = "internal.llm.call")]
    LlmCall,
    // Connectors
    #[serde(rename = "incoming.message")]
    IncomingMessage,
    #[serde(rename = "outgoing.message")]
    OutgoingMessage,
    #[serde(rename = "connector.message")]
    ConnectorMessage,
    #[serde(rename = "connector.reply")]
    ConnectorReply,
    #[serde(rename = "connector.typing")]
    ConnectorTyping,
    #[serde(rename = "connector.add_reaction")]
    ConnectorAddReaction,
    #[serde(rename = "connector.clear_reactions")]
    ConnectorClearReactions,
    // Flow control
    #[serde(rename = "agent.cancelled")]
    AgentCancelled,
    #[serde(rename = "agent.yielded")]
    AgentYielded,
    // Schedules
    #[serde(rename = "schedule.trigger")]
    ScheduleTrigger,
    #[serde(rename = "schedule.created")]
    ScheduleCreated,
    #[serde(rename = "schedule.removed")]
    ScheduleRemoved,
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
    // Dream
    #[serde(rename = "dream.completed")]
    DreamCompleted,
    // Pairing
    #[serde(rename = "pairing.request.device")]
    PairingRequestDevice,
    #[serde(rename = "pairing.request.chat")]
    PairingRequestChat,
    #[serde(rename = "pairing.approved.device")]
    PairingApprovedDevice,
    #[serde(rename = "pairing.approved.chat")]
    PairingApprovedChat,
    #[serde(rename = "pairing.rejected")]
    PairingRejected,
    // Error
    #[serde(rename = "error")]
    Error,
}

impl EventKind {
    /// Returns the wire string for this event kind (zero allocation).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user.message",
            Self::AssistantMessage => "assistant.message",
            Self::AssistantStream => "assistant.stream",
            Self::ToolCall => "tool.call",
            Self::ToolApproved => "tool.approved",
            Self::ToolProgress => "tool.progress",
            Self::ToolResult => "tool.result",
            Self::PromptRequest => "prompt.request",
            Self::PromptResponse => "prompt.response",
            Self::SessionCreated => "session.created",
            Self::SessionClosed => "session.closed",
            Self::SessionClear => "session.clear",
            Self::LlmCall => "internal.llm.call",
            Self::IncomingMessage => "incoming.message",
            Self::OutgoingMessage => "outgoing.message",
            Self::ConnectorMessage => "connector.message",
            Self::ConnectorReply => "connector.reply",
            Self::ConnectorTyping => "connector.typing",
            Self::ConnectorAddReaction => "connector.add_reaction",
            Self::ConnectorClearReactions => "connector.clear_reactions",
            Self::AgentCancelled => "agent.cancelled",
            Self::AgentYielded => "agent.yielded",
            Self::ScheduleTrigger => "schedule.trigger",
            Self::ScheduleCreated => "schedule.created",
            Self::ScheduleRemoved => "schedule.removed",
            Self::SkillStarted => "skill.started",
            Self::SkillCompleted => "skill.completed",
            Self::SkillStepStarted => "skill.step.started",
            Self::SkillStepCompleted => "skill.step.completed",
            Self::ContextLayered => "context.layered",
            Self::DreamCompleted => "dream.completed",
            Self::PairingRequestDevice => "pairing.request.device",
            Self::PairingRequestChat => "pairing.request.chat",
            Self::PairingApprovedDevice => "pairing.approved.device",
            Self::PairingApprovedChat => "pairing.approved.chat",
            Self::PairingRejected => "pairing.rejected",
            Self::Error => "error",
        }
    }

    /// Parses a wire string into an `EventKind`, if known.
    pub fn parse(s: &str) -> Option<Self> {
        // Use serde for parsing — the rename tags are the source of truth.
        #[derive(Deserialize)]
        struct Tag {
            #[serde(rename = "type")]
            kind: EventKind,
        }
        serde_json::from_value::<Tag>(serde_json::json!({"type": s}))
            .ok()
            .map(|t| t.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_parse() {
        let kinds = [
            EventKind::UserMessage,
            EventKind::AssistantMessage,
            EventKind::AssistantStream,
            EventKind::ToolCall,
            EventKind::PromptRequest,
            EventKind::PromptResponse,
            EventKind::SessionCreated,
            EventKind::AgentCancelled,
            EventKind::AgentYielded,
            EventKind::SkillStepStarted,
            EventKind::Error,
        ];
        for kind in kinds {
            let wire = kind.as_str();
            let parsed = EventKind::parse(wire)
                .unwrap_or_else(|| panic!("parse failed for: {wire}"));
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn parse_unknown() {
        assert!(EventKind::parse("nonexistent.event").is_none());
    }
}
