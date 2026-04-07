use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::Message;

/// Lifecycle status of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Session is actively in use.
    #[default]
    Active,
    /// Session has been explicitly closed.
    Closed,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

/// Cumulative token usage for a session.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionTokenUsage {
    /// Total input tokens consumed.
    #[serde(default)]
    pub input: u64,
    /// Total output tokens consumed.
    #[serde(default)]
    pub output: u64,
}

impl SessionTokenUsage {
    /// Returns `true` if both input and output are zero.
    pub fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0
    }
}

/// An active conversation session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Lifecycle status (active or closed).
    #[serde(default)]
    pub status: SessionStatus,
    /// LLM model name used in this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Working directory for this session.
    #[serde(default)]
    pub root_dir: Option<String>,
    /// Compressed summary of earlier turns.
    #[serde(default)]
    pub summary: Option<String>,
    /// Message index up to which the summary covers.
    #[serde(default)]
    pub summary_up_to: usize,
    /// Preferred language for this session (e.g. "fr", "en").
    #[serde(default)]
    pub language: Option<String>,
    /// Human-readable session title.
    #[serde(default)]
    pub title: Option<String>,
    /// Total number of messages in this session.
    #[serde(default)]
    pub message_count: usize,
    /// Cumulative token usage (input + output) for cost tracking.
    #[serde(default, skip_serializing_if = "SessionTokenUsage::is_zero")]
    pub token_usage: SessionTokenUsage,
    /// Tools approved for dangerous execution in this session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tools: Vec<String>,
    /// Arbitrary key-value metadata (e.g. connector, platform, user_id).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// Policy name governing tool access for this session (set by connector pairing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_name: Option<String>,
}

impl Session {
    /// Creates a new session with sensible defaults.
    /// Only `id` is required; all other fields use defaults.
    pub fn new(id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            created_at: now,
            updated_at: now,
            status: SessionStatus::default(),
            model: None,
            root_dir: None,
            summary: None,
            summary_up_to: 0,
            language: None,
            title: None,
            message_count: 0,
            token_usage: SessionTokenUsage::default(),
            approved_tools: Vec::new(),
            metadata: HashMap::new(),
            policy_name: None,
        }
    }

    /// Returns `true` if the session is active.
    pub fn is_active(&self) -> bool {
        self.status == SessionStatus::Active
    }
}

/// Persistence interface for sessions.
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    async fn create(&self, session: &Session) -> Result<(), SessionError>;
    async fn get(&self, id: &str) -> Result<Option<Session>, SessionError>;
    async fn update(&self, session: &Session) -> Result<(), SessionError>;
    async fn delete(&self, id: &str) -> Result<(), SessionError>;
    async fn list(&self) -> Result<Vec<Session>, SessionError>;

    /// Marks a session as closed.
    async fn close(&self, id: &str) -> Result<(), SessionError> {
        let mut session = self
            .get(id)
            .await?
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;
        session.status = SessionStatus::Closed;
        session.updated_at = Utc::now();
        self.update(&session).await
    }

    /// Appends a message to a session's history.
    async fn append_message(&self, session_id: &str, msg: Message) -> Result<(), SessionError>;
    /// Loads the full message history for a session.
    async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>, SessionError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}
