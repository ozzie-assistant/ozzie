use std::path::Path;

use tracing::debug;

use super::catalog::{
    AGENT_INSTRUCTIONS, AGENT_INSTRUCTIONS_COMPACT, DEFAULT_PERSONA, DEFAULT_PERSONA_COMPACT,
    SUB_AGENT_INSTRUCTIONS, SUB_AGENT_INSTRUCTIONS_COMPACT,
};
use crate::domain::ModelTier;

/// Loads the persona from SOUL.md if present, otherwise returns DefaultPersona.
pub fn load_persona(ozzie_path: &Path) -> String {
    let soul_path = ozzie_path.join("SOUL.md");
    if soul_path.exists() {
        match std::fs::read_to_string(&soul_path) {
            Ok(content) if !content.trim().is_empty() => {
                debug!(path = %soul_path.display(), "loaded custom persona from SOUL.md");
                return content;
            }
            Ok(_) => {
                debug!("SOUL.md is empty, using default persona");
            }
            Err(e) => {
                debug!(error = %e, "failed to read SOUL.md, using default persona");
            }
        }
    }
    DEFAULT_PERSONA.to_string()
}

/// Returns the appropriate persona for the given model tier.
/// Compact persona for Small tier, full persona otherwise.
pub fn persona_for_tier(full: &str, tier: ModelTier) -> &str {
    match tier {
        ModelTier::Small => DEFAULT_PERSONA_COMPACT,
        _ => full,
    }
}

/// Returns agent instructions appropriate for the given model tier.
pub fn agent_instructions_for_tier(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Small => AGENT_INSTRUCTIONS_COMPACT,
        _ => AGENT_INSTRUCTIONS,
    }
}

/// Returns sub-agent instructions appropriate for the given model tier.
pub fn sub_agent_instructions_for_tier(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Small => SUB_AGENT_INSTRUCTIONS_COMPACT,
        _ => SUB_AGENT_INSTRUCTIONS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_persona_default_when_no_soul() {
        let dir = tempfile::tempdir().unwrap();
        let persona = load_persona(dir.path());
        assert!(persona.contains("Ozzie"));
        assert!(persona.contains("Simplicity first"));
    }

    #[test]
    fn load_persona_from_soul_md() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "Custom persona text").unwrap();
        let persona = load_persona(dir.path());
        assert_eq!(persona, "Custom persona text");
    }

    #[test]
    fn load_persona_empty_soul_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "   ").unwrap();
        let persona = load_persona(dir.path());
        assert!(persona.contains("Ozzie"));
    }

    #[test]
    fn persona_for_tier_small_returns_compact() {
        let result = persona_for_tier("full persona", ModelTier::Small);
        assert_eq!(result, DEFAULT_PERSONA_COMPACT);
    }

    #[test]
    fn persona_for_tier_large_returns_full() {
        let result = persona_for_tier("full persona", ModelTier::Large);
        assert_eq!(result, "full persona");
    }

    #[test]
    fn agent_instructions_for_tier_variants() {
        assert!(agent_instructions_for_tier(ModelTier::Large).contains("Parallel Execution"));
        assert!(agent_instructions_for_tier(ModelTier::Small).contains("Primary user interface"));
    }

    #[test]
    fn sub_agent_instructions_for_tier_variants() {
        assert!(sub_agent_instructions_for_tier(ModelTier::Large).contains("file_write"));
        assert!(sub_agent_instructions_for_tier(ModelTier::Small).contains("Task execution agent"));
    }
}
