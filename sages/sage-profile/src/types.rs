use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Provenance of a whoami entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhoamiSource {
    /// Provided during onboarding — protected from consolidation.
    Intro,
    /// Extracted from conversation by the agent.
    Conversation,
    /// Produced by LLM consolidation of multiple entries.
    Consolidated,
}

/// User profile — identity-level knowledge about the user.
///
/// Designed to be loaded into an LLM system prompt on every interaction.
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
    /// Agent's knowledge about the user — identity-level facts only.
    ///
    /// Entries with [`WhoamiSource::Intro`] are protected from consolidation.
    /// Other entries may be consolidated by an LLM when the list grows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub whoami: Vec<WhoamiEntry>,
    /// When the profile was first created.
    pub created_at: NaiveDate,
    /// When the profile was last updated.
    pub updated_at: NaiveDate,
}

/// A single piece of knowledge the agent holds about the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiEntry {
    /// The information itself — a concise statement.
    pub info: String,
    /// When this entry was created.
    pub created_at: NaiveDate,
    /// Provenance of this entry.
    pub source: WhoamiSource,
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
                source: WhoamiSource::Intro,
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
    ///
    /// Skips if an entry with the same `info` text already exists (any source).
    pub fn add_observation(&mut self, info: String) {
        if self.whoami.iter().any(|e| e.info == info) {
            return;
        }
        let today = chrono::Utc::now().date_naive();
        self.whoami.push(WhoamiEntry {
            info,
            created_at: today,
            source: WhoamiSource::Conversation,
        });
        self.updated_at = today;
    }

    /// Returns all intro entries (protected from consolidation).
    pub fn intro_entries(&self) -> Vec<&WhoamiEntry> {
        self.whoami
            .iter()
            .filter(|e| e.source == WhoamiSource::Intro)
            .collect()
    }

    /// Returns all non-intro entries (eligible for consolidation).
    pub fn compressible_entries(&self) -> Vec<&WhoamiEntry> {
        self.whoami
            .iter()
            .filter(|e| e.source != WhoamiSource::Intro)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> UserProfile {
        UserProfile::new("Test".into(), vec!["intro fact".into()])
    }

    #[test]
    fn new_profile_has_intro_entry() {
        let p = test_profile();
        assert_eq!(p.whoami.len(), 1);
        assert_eq!(p.whoami[0].source, WhoamiSource::Intro);
    }

    #[test]
    fn add_observation_deduplicates() {
        let mut p = test_profile();
        p.add_observation("likes Rust".into());
        p.add_observation("likes Rust".into());
        assert_eq!(p.whoami.len(), 2);
    }

    #[test]
    fn add_observation_skips_existing_intro() {
        let mut p = test_profile();
        p.add_observation("intro fact".into());
        assert_eq!(p.whoami.len(), 1);
    }

    #[test]
    fn add_observation_allows_different_entries() {
        let mut p = test_profile();
        p.add_observation("fact A".into());
        p.add_observation("fact B".into());
        assert_eq!(p.whoami.len(), 3);
    }

    #[test]
    fn intro_entries_returns_only_intro() {
        let mut p = test_profile();
        p.add_observation("from conversation".into());
        assert_eq!(p.intro_entries().len(), 1);
        assert_eq!(p.compressible_entries().len(), 1);
    }

    #[test]
    fn serde_roundtrip() {
        let p = test_profile();
        let json = serde_json::to_string(&p).expect("serialize");
        let p2: UserProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p2.name, "Test");
        assert_eq!(p2.whoami.len(), 1);
    }
}
