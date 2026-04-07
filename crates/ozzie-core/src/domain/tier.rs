use serde::{Deserialize, Serialize};

/// Classifies LLM capabilities for prompt adaptation.
/// Only `Small` triggers compact prompt variants; `Medium` and `Large`
/// share the full prompts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    /// < 16K context
    Small,
    /// 16K-64K context
    Medium,
    /// >= 64K context
    #[default]
    Large,
}

impl ModelTier {
    /// Returns the model tier. An explicit tier string (from config) takes
    /// precedence; otherwise the context window size is used.
    pub fn resolve(explicit_tier: Option<&str>, context_window: usize) -> Self {
        if let Some(tier) = explicit_tier {
            match tier {
                "small" => return Self::Small,
                "medium" => return Self::Medium,
                "large" => return Self::Large,
                _ => {}
            }
        }

        match context_window {
            0 => Self::Large,
            w if w < 16_000 => Self::Small,
            w if w < 64_000 => Self::Medium,
            _ => Self::Large,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_tier_takes_precedence() {
        assert_eq!(ModelTier::resolve(Some("small"), 100_000), ModelTier::Small);
        assert_eq!(ModelTier::resolve(Some("medium"), 1_000), ModelTier::Medium);
        assert_eq!(ModelTier::resolve(Some("large"), 1_000), ModelTier::Large);
    }

    #[test]
    fn context_window_resolution() {
        assert_eq!(ModelTier::resolve(None, 8_000), ModelTier::Small);
        assert_eq!(ModelTier::resolve(None, 15_999), ModelTier::Small);
        assert_eq!(ModelTier::resolve(None, 16_000), ModelTier::Medium);
        assert_eq!(ModelTier::resolve(None, 32_000), ModelTier::Medium);
        assert_eq!(ModelTier::resolve(None, 63_999), ModelTier::Medium);
        assert_eq!(ModelTier::resolve(None, 64_000), ModelTier::Large);
        assert_eq!(ModelTier::resolve(None, 128_000), ModelTier::Large);
    }

    #[test]
    fn zero_context_defaults_to_large() {
        assert_eq!(ModelTier::resolve(None, 0), ModelTier::Large);
    }

    #[test]
    fn unknown_explicit_tier_falls_through() {
        assert_eq!(ModelTier::resolve(Some("unknown"), 8_000), ModelTier::Small);
    }
}
