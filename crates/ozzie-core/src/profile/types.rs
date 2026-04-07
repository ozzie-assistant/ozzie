use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// User profile — identity-level knowledge about the user.
///
/// Loaded into the system prompt on every interaction.
/// Must stay compact (a few hundred tokens max).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// User's name.
    pub name: String,
    /// Preferred communication tone (e.g. "casual, direct, en francais").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    /// Preferred language (e.g. "fr", "en").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Ozzie's knowledge about the user — identity-level facts only.
    ///
    /// Entries with `source: "intro"` are protected from compression.
    /// Other entries may be consolidated by an LLM when the list grows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub whoami: Vec<WhoamiEntry>,
    /// When the profile was first created.
    pub created_at: NaiveDate,
    /// When the profile was last updated.
    pub updated_at: NaiveDate,
}

/// A single piece of knowledge Ozzie holds about the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiEntry {
    /// The information itself — a concise statement.
    pub info: String,
    /// When this entry was created.
    pub created_at: NaiveDate,
    /// Provenance: "intro" (protected), "conversation", "consolidated".
    pub source: String,
}

impl UserProfile {
    /// Creates a new profile with the given name and intro entries.
    pub fn new(name: String, intro_entries: Vec<String>) -> Self {
        let today = chrono::Utc::now().date_naive();
        let whoami = intro_entries
            .into_iter()
            .map(|info| WhoamiEntry {
                info,
                created_at: today,
                source: "intro".to_string(),
            })
            .collect();

        Self {
            name,
            tone: None,
            language: None,
            whoami,
            created_at: today,
            updated_at: today,
        }
    }

    /// Adds a whoami entry from a conversation observation.
    pub fn add_observation(&mut self, info: String) {
        let today = chrono::Utc::now().date_naive();
        self.whoami.push(WhoamiEntry {
            info,
            created_at: today,
            source: "conversation".to_string(),
        });
        self.updated_at = today;
    }

    /// Returns all intro entries (protected from compression).
    pub fn intro_entries(&self) -> Vec<&WhoamiEntry> {
        self.whoami.iter().filter(|e| e.source == "intro").collect()
    }

    /// Returns all non-intro entries (eligible for compression).
    pub fn compressible_entries(&self) -> Vec<&WhoamiEntry> {
        self.whoami
            .iter()
            .filter(|e| e.source != "intro")
            .collect()
    }
}
