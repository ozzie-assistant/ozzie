use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Standard message roles.
pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";
pub const ROLE_SYSTEM: &str = "system";
pub const ROLE_TOOL: &str = "tool";

fn default_true() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
}

/// Domain-level chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// Timestamp; None when not applicable (e.g. LLM prompts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<DateTime<Utc>>,
    /// Whether this message is shown to the user in the UI. Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub user_visible: bool,
    /// Whether this message is sent to the LLM. Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub agent_visible: bool,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            ts: Some(Utc::now()),
            user_visible: true,
            agent_visible: true,
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

    pub fn with_user_visible(mut self, v: bool) -> Self {
        self.user_visible = v;
        self
    }

    pub fn with_agent_visible(mut self, v: bool) -> Self {
        self.agent_visible = v;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_visibility_is_true() {
        let msg = Message::user("hello");
        assert!(msg.user_visible);
        assert!(msg.agent_visible);
    }

    #[test]
    fn serde_backward_compat() {
        // Old JSON without visibility fields should deserialize with defaults
        let json = r#"{"role":"user","content":"hi"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(msg.user_visible);
        assert!(msg.agent_visible);
    }

    #[test]
    fn serde_roundtrip_with_visibility() {
        let msg = Message::user("hi").with_agent_visible(false);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("agent_visible"));
        assert!(!json.contains("user_visible")); // true is skipped

        let back: Message = serde_json::from_str(&json).unwrap();
        assert!(back.user_visible);
        assert!(!back.agent_visible);
    }

    #[test]
    fn builder_methods() {
        let msg = Message::assistant("result")
            .with_user_visible(false)
            .with_agent_visible(true);
        assert!(!msg.user_visible);
        assert!(msg.agent_visible);
    }
}
