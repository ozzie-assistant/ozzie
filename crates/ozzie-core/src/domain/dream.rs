use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tracks what was extracted from a session during a dream consolidation run.
///
/// Each record is incremental: `consolidated_up_to` marks the message index boundary.
/// On re-run, only messages beyond this index are processed, with previous extractions
/// passed as context to avoid duplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamRecord {
    /// Session this record belongs to.
    pub session_id: String,
    /// Message index up to which we have processed (exclusive).
    pub consolidated_up_to: usize,
    /// Profile entries (whoami texts) extracted across all runs for this session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile_entries: Vec<String>,
    /// Memory IDs created across all runs for this session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_ids: Vec<String>,
    /// When this record was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Result of classifying a session's messages via LLM.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DreamExtraction {
    /// Identity-level facts to add to user profile.
    #[serde(default)]
    pub profile: Vec<String>,
    /// Contextual knowledge to store as semantic memories.
    #[serde(default)]
    pub memory: Vec<DreamMemoryEntry>,
}

/// A memory entry extracted by the dream classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamMemoryEntry {
    pub title: String,
    pub content: String,
    /// One of: preference, fact, procedure, context.
    #[serde(rename = "type")]
    pub memory_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Statistics from a single dream run.
#[derive(Debug, Clone, Default)]
pub struct DreamStats {
    pub sessions_processed: usize,
    pub sessions_skipped: usize,
    pub sessions_errored: usize,
    pub profile_entries_added: usize,
    pub memories_created: usize,
}

impl std::fmt::Display for DreamStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "processed={}, skipped={}, errors={}, profile+={}, memories+={}",
            self.sessions_processed,
            self.sessions_skipped,
            self.sessions_errored,
            self.profile_entries_added,
            self.memories_created,
        )
    }
}
