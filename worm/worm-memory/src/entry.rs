use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Categorizes a memory entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    Preference,
    Fact,
    Procedure,
    Context,
}

impl MemoryType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Preference => "preference",
            Self::Fact => "fact",
            Self::Procedure => "procedure",
            Self::Context => "context",
        }
    }
}

impl std::str::FromStr for MemoryType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "preference" => Ok(Self::Preference),
            "fact" => Ok(Self::Fact),
            "procedure" => Ok(Self::Procedure),
            "context" => Ok(Self::Context),
            _ => Err(format!("unknown memory type: {s}")),
        }
    }
}

/// Holds metadata for a single memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub title: String,
    pub source: String,
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub confidence: f64,
    #[serde(default)]
    pub importance: ImportanceLevel,

    /// Embedding model used for last indexing. Empty = never indexed.
    #[serde(default)]
    pub embedding_model: String,
    /// When the entry was last indexed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_at: Option<DateTime<Utc>>,

    /// Target memory ID when this entry was consolidated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
}

impl MemoryEntry {
    /// Returns true if this entry has been indexed with embeddings.
    pub fn is_indexed(&self) -> bool {
        !self.embedding_model.is_empty() && self.indexed_at.is_some()
    }

    /// Returns true if the entry content was updated after last indexing,
    /// or if the embedding model has changed.
    pub fn is_stale(&self, current_model: &str) -> bool {
        if !self.is_indexed() {
            return true;
        }
        if self.embedding_model != current_model {
            return true;
        }
        match self.indexed_at {
            Some(indexed) => indexed < self.updated_at,
            None => true,
        }
    }
}

/// Controls how aggressively a memory decays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportanceLevel {
    /// Never decays.
    Core,
    /// Slow decay.
    Important,
    /// Default decay.
    #[default]
    Normal,
    /// Fast decay, auto-purge.
    Ephemeral,
}

impl ImportanceLevel {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Core => "core",
            Self::Important => "important",
            Self::Normal => "normal",
            Self::Ephemeral => "ephemeral",
        }
    }

    pub fn is_valid(s: &str) -> bool {
        s.parse::<Self>().is_ok()
    }
}

impl std::str::FromStr for ImportanceLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "core" => Ok(Self::Core),
            "important" => Ok(Self::Important),
            "normal" => Ok(Self::Normal),
            "ephemeral" => Ok(Self::Ephemeral),
            _ => Err(format!("unknown importance level: {s}")),
        }
    }
}
