use serde::{Deserialize, Serialize};

/// Semantic reaction type for connector status indicators.
///
/// Each connector maps these to platform-specific representations
/// (emoji, icon, status indicator, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reaction {
    /// LLM is reasoning.
    Thinking,
    /// Generic tool call in progress.
    Tool,
    /// Web search or fetch.
    Web,
    /// Shell command execution.
    Command,
    /// File editing.
    Edit,
    /// Task management.
    Task,
    /// Memory operations.
    Memory,
    /// Scheduling.
    Schedule,
    /// Tool/skill activation.
    Activate,
}

impl Reaction {
    /// All variants, for iteration (e.g. clearing all own reactions).
    pub const ALL: &'static [Self] = &[
        Self::Thinking,
        Self::Tool,
        Self::Web,
        Self::Command,
        Self::Edit,
        Self::Task,
        Self::Memory,
        Self::Schedule,
        Self::Activate,
    ];
}

/// Option in a prompt request (e.g. "Allow once", "Deny").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOption {
    pub value: String,
    pub label: String,
}

/// A chat message (role + content) for wire transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    pub role: String,
    pub content: String,
}

/// Per-tool argument constraints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConstraint {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_domains: Vec<String>,
}
