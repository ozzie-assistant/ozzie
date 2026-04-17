use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Standard message roles.
pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";

/// A chat message for context compression.
///
/// Minimal representation — consumers convert from their own message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
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
}
