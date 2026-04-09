use serde::{Deserialize, Serialize};

use crate::common::MessagePayload;

// ---- Request params ----

/// Parameters for `open_session`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenSessionParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// An image attachment in a `send_message` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type (e.g. "image/png", "image/jpeg").
    pub media_type: String,
    /// Optional alt text for accessibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

/// Parameters for `send_message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    pub session_id: String,
    pub text: String,
    /// Optional image attachments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageAttachment>,
}

/// Parameters for `load_messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadMessagesParams {
    pub session_id: String,
    #[serde(default = "default_load_limit")]
    pub limit: u64,
}

fn default_load_limit() -> u64 {
    10
}

/// Parameters for `accept_all_tools`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptAllToolsParams {
    pub session_id: String,
}

/// Parameters for `send_connector_message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendConnectorMessageParams {
    /// Connector name (e.g. "discord", "file").
    pub connector: String,
    /// Channel within the connector.
    pub channel_id: String,
    /// Author display name from the connector platform.
    pub author: String,
    /// Message text content.
    pub content: String,
    /// Optional platform-specific message ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Parameters for `cancel_session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelSessionParams {
    pub session_id: String,
}

/// Parameters for `prompt_response`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponseParams {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// ---- Response results ----

/// Result for `open_session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_dir: Option<String>,
}

/// Generic accepted result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptedResult {
    pub accepted: bool,
}

/// Result for `cancel_session`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledResult {
    pub cancelled: bool,
}

/// Result for `load_messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResult {
    pub messages: Vec<MessagePayload>,
}
