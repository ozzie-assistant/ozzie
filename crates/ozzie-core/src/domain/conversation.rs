use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::Message;

/// Lifecycle status of a conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConversationStatus {
    /// Conversation is actively in use.
    #[default]
    Active,
    /// Conversation has been archived (frozen + hidden, history preserved).
    Archived,
}

impl std::fmt::Display for ConversationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Archived => write!(f, "archived"),
        }
    }
}

/// Cumulative token usage for a conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConversationTokenUsage {
    /// Total input tokens consumed.
    #[serde(default)]
    pub input: u64,
    /// Total output tokens consumed.
    #[serde(default)]
    pub output: u64,
}

impl ConversationTokenUsage {
    /// Returns `true` if both input and output are zero.
    pub fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0
    }
}

/// A conversation — a topical thread of exchanges with the agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Conversation {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Lifecycle status (active or archived).
    #[serde(default)]
    pub status: ConversationStatus,
    /// LLM model name used in this conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Working directory for this conversation.
    #[serde(default)]
    pub root_dir: Option<String>,
    /// Compressed summary of earlier turns.
    #[serde(default)]
    pub summary: Option<String>,
    /// Message index up to which the summary covers.
    #[serde(default)]
    pub summary_up_to: usize,
    /// Preferred language for this conversation (e.g. "fr", "en").
    #[serde(default)]
    pub language: Option<String>,
    /// Human-readable conversation title.
    #[serde(default)]
    pub title: Option<String>,
    /// Total number of messages in this conversation.
    #[serde(default)]
    pub message_count: usize,
    /// Cumulative token usage (input + output) for cost tracking.
    #[serde(default, skip_serializing_if = "ConversationTokenUsage::is_zero")]
    pub token_usage: ConversationTokenUsage,
    /// Tools approved for dangerous execution in this conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tools: Vec<String>,
    /// Arbitrary key-value metadata (e.g. connector, platform, user_id).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Policy name governing tool access for this conversation (set by connector pairing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_name: Option<String>,
    /// Active project for this conversation (set by `open_project` tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl Conversation {
    /// Creates a new conversation with sensible defaults.
    /// Only `id` is required; all other fields use defaults.
    pub fn new(id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            created_at: now,
            updated_at: now,
            status: ConversationStatus::default(),
            model: None,
            root_dir: None,
            summary: None,
            summary_up_to: 0,
            language: None,
            title: None,
            message_count: 0,
            token_usage: ConversationTokenUsage::default(),
            approved_tools: Vec::new(),
            metadata: HashMap::new(),
            policy_name: None,
            project_id: None,
        }
    }

    /// Returns `true` if the conversation is active.
    pub fn is_active(&self) -> bool {
        self.status == ConversationStatus::Active
    }
}

/// Persistence interface for conversations.
#[async_trait::async_trait]
pub trait ConversationStore: Send + Sync {
    async fn create(&self, conversation: &Conversation) -> Result<(), ConversationError>;
    async fn get(&self, id: &str) -> Result<Option<Conversation>, ConversationError>;
    async fn update(&self, conversation: &Conversation) -> Result<(), ConversationError>;
    async fn delete(&self, id: &str) -> Result<(), ConversationError>;
    async fn list(&self) -> Result<Vec<Conversation>, ConversationError>;

    /// Archives a conversation (freezes + hides, history preserved).
    async fn archive(&self, id: &str) -> Result<(), ConversationError> {
        let mut conversation = self
            .get(id)
            .await?
            .ok_or_else(|| ConversationError::NotFound(id.to_string()))?;
        conversation.status = ConversationStatus::Archived;
        conversation.updated_at = Utc::now();
        self.update(&conversation).await
    }

    /// Appends a message to a conversation's history.
    async fn append_message(
        &self,
        conversation_id: &str,
        msg: Message,
    ) -> Result<(), ConversationError>;
    /// Loads the full message history for a conversation.
    async fn load_messages(&self, conversation_id: &str) -> Result<Vec<Message>, ConversationError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("conversation not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}

/// Read-only view of a conversation for listing purposes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationSummary {
    pub id: String,
    pub title: Option<String>,
    pub status: ConversationStatus,
    pub message_count: usize,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
}
