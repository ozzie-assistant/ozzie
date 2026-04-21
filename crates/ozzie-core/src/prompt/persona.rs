use super::catalog::{
    AGENT_INSTRUCTIONS, AGENT_INSTRUCTIONS_COMPACT, DEFAULT_PERSONA, DEFAULT_PERSONA_COMPACT,
    SUB_AGENT_INSTRUCTIONS, SUB_AGENT_INSTRUCTIONS_COMPACT,
};
use crate::domain::ModelTier;

/// Returns the persona text.
///
/// Uses `custom_persona` if provided and non-empty, otherwise returns the default.
/// The caller is responsible for loading the custom persona from disk (e.g. SOUL.md).
pub fn load_persona(custom_persona: Option<&str>) -> String {
    match custom_persona {
        Some(text) if !text.trim().is_empty() => text.to_string(),
        _ => DEFAULT_PERSONA.to_string(),
    }
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
    fn load_persona_default_when_none() {
        let persona = load_persona(None);
        assert!(persona.contains("Ozzie"));
        assert!(persona.contains("Simplicity first"));
    }

    #[test]
    fn load_persona_custom() {
        let persona = load_persona(Some("Custom persona text"));
        assert_eq!(persona, "Custom persona text");
    }

    #[test]
    fn load_persona_empty_falls_back() {
        let persona = load_persona(Some("   "));
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
