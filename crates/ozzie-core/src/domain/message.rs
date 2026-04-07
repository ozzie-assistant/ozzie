use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Standard message roles.
pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_SYSTEM: &str = "system";
pub const ROLE_TOOL: &str = "tool";

/// Domain-level chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// Timestamp; None when not applicable (e.g. LLM prompts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<DateTime<Utc>>,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            ts: Some(Utc::now()),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ROLE_USER, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ROLE_ASSISTANT, content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ROLE_SYSTEM, content)
    }
}
