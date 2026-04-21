use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tracks what was extracted from a session during a dream consolidation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamRecord {
    pub session_id: String,
    /// Message index up to which we have processed (exclusive).
    pub consolidated_up_to: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile_entries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_ids: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

/// Result of classifying a session's messages via LLM.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DreamExtraction {
    #[serde(default)]
    pub profile: Vec<String>,
    #[serde(default)]
    pub memory: Vec<DreamMemoryEntry>,
}

/// A memory entry extracted by the dream classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamMemoryEntry {
    pub title: String,
    pub content: String,
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
    pub pages_created: usize,
    pub pages_updated: usize,
}

impl std::fmt::Display for DreamStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "processed={}, skipped={}, errors={}, profile+={}, memories+={}, pages+={}/~{}",
            self.sessions_processed,
            self.sessions_skipped,
            self.sessions_errored,
            self.profile_entries_added,
            self.memories_created,
            self.pages_created,
            self.pages_updated,
        )
    }
}

/// Statistics from a synthesis pass.
#[derive(Debug, Default)]
pub struct SynthesisStats {
    pub pages_created: usize,
    pub pages_updated: usize,
    pub pages_split: usize,
    pub clusters_skipped: usize,
}

/// Tracks consolidation progress for a project workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRecord {
    pub project_name: String,
    /// Git commit SHA at last scan (watermark).
    pub last_commit: String,
    /// Memory entry IDs created from this workspace.
    pub memory_ids: Vec<String>,
    pub updated_at: DateTime<Utc>,
}
