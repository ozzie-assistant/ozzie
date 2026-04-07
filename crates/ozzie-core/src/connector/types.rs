use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An external user on a platform.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Identity {
    /// Platform name: "discord", "slack", "webhook", etc.
    pub platform: String,
    /// Platform-specific user ID.
    pub user_id: String,
    /// Display name.
    pub name: String,
    /// Guild/workspace ID (empty for DMs or platforms without servers).
    #[serde(default)]
    pub server_id: String,
    /// Channel/conversation ID.
    #[serde(default)]
    pub channel_id: String,
}

/// A message received from an external platform.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncomingMessage {
    /// Who sent the message.
    pub identity: Identity,
    /// Text content.
    pub content: String,
    /// Channel where the message was received (falls back to `identity.channel_id`).
    #[serde(default)]
    pub channel_id: String,
    /// Platform-specific message ID (for replies, reactions).
    #[serde(default)]
    pub message_id: String,
    /// When the message was sent.
    pub timestamp: DateTime<Utc>,
    /// Platform-specific structured data (attachments, embeds, etc.).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Parsed slash command, if any (e.g. "pair", "setup", "status").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments following the command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_args: Vec<String>,
    /// Platform roles resolved at message time (opaque strings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// True when this is a direct message (not a channel message).
    #[serde(default)]
    pub is_dm: bool,
}

/// A message to send to an external platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMessage {
    /// Text content.
    pub content: String,
    /// Target channel.
    pub channel_id: String,
    /// Reply to a specific message (threading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    /// Platform-specific structured data (embeds, buttons, etc.).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

// Re-export Reaction from ozzie-types (single source of truth).
pub use ozzie_types::Reaction;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_serde_roundtrip() {
        let id = Identity {
            platform: "discord".to_string(),
            user_id: "123".to_string(),
            name: "Alice".to_string(),
            server_id: "guild_1".to_string(),
            channel_id: "ch_1".to_string(),
        };
        let json = serde_json::to_string(&id).unwrap();
        let back: Identity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.platform, "discord");
        assert_eq!(back.user_id, "123");
    }

    #[test]
    fn incoming_message_metadata() {
        let msg = IncomingMessage {
            identity: Identity {
                platform: "discord".to_string(),
                user_id: "u1".to_string(),
                name: "Bob".to_string(),
                server_id: String::new(),
                channel_id: "dm".to_string(),
            },
            content: "hello".to_string(),
            channel_id: "dm".to_string(),
            message_id: "msg_1".to_string(),
            timestamp: Utc::now(),
            metadata: {
                let mut m = HashMap::new();
                m.insert(
                    "attachments".to_string(),
                    serde_json::json!([{"url": "https://example.com/file.png"}]),
                );
                m
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("attachments"));
    }

    #[test]
    fn outgoing_message_minimal() {
        let msg = OutgoingMessage {
            content: "reply".to_string(),
            channel_id: "ch_1".to_string(),
            reply_to_id: None,
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        // metadata and reply_to_id should be skipped
        assert!(!json.contains("metadata"));
        assert!(!json.contains("reply_to_id"));
    }

    #[test]
    fn reaction_serde() {
        let r = Reaction::Thinking;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "\"thinking\"");

        let back: Reaction = serde_json::from_str("\"web\"").unwrap();
        assert_eq!(back, Reaction::Web);
    }
}
