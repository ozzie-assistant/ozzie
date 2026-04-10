use serde::{Deserialize, Serialize};

// ---- Multimodal content ----

/// A reference to a blob stored on disk (e.g. in `{session_dir}/blobs/{hash}.{ext}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobRef {
    /// Content-addressed hash (SHA-256 hex).
    pub hash: String,
    /// MIME type (e.g. "image/png", "image/jpeg").
    pub media_type: String,
}

/// A content part in a message — text or image reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image {
        blob: BlobRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
    /// Image with base64 data pre-loaded. Transient — used only for LLM API calls.
    /// Created by `resolve_blobs()`, never persisted to disk.
    ImageInline {
        media_type: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
    },
}

impl ContentPart {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    pub fn image(blob: BlobRef) -> Self {
        Self::Image { blob, alt: None }
    }

    pub fn image_with_alt(blob: BlobRef, alt: impl Into<String>) -> Self {
        Self::Image { blob, alt: Some(alt.into()) }
    }

    pub fn image_inline(media_type: impl Into<String>, base64_data: impl Into<String>) -> Self {
        Self::ImageInline {
            media_type: media_type.into(),
            data: base64_data.into(),
            alt: None,
        }
    }

    /// Returns the text content if this is a `Text` part, or `None`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns true if this is any image variant (Image or ImageInline).
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. } | Self::ImageInline { .. })
    }
}

/// Convenience: collapse `Vec<ContentPart>` into a single text string (ignoring non-text parts).
pub fn parts_to_text(parts: &[ContentPart]) -> String {
    let texts: Vec<&str> = parts.iter().filter_map(|p| p.as_text()).collect();
    texts.join("\n")
}

/// Convenience: wrap a text string into a single-element content parts vec.
pub fn text_to_parts(text: impl Into<String>) -> Vec<ContentPart> {
    vec![ContentPart::text(text)]
}

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
    /// Whether this message is shown to the user in the UI. Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub user_visible: bool,
    /// Whether this message is sent to the LLM. Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub agent_visible: bool,
}

fn default_true() -> bool {
    true
}
fn is_true(v: &bool) -> bool {
    *v
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
