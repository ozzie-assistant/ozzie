use serde::{Deserialize, Serialize};

/// Model capability — describes what a model can do.
///
/// Restored from the original Go enum (`internal/models/capability.go`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    /// Extended thinking / chain-of-thought.
    Thinking,
    /// Image/multimodal input.
    Vision,
    /// Function/tool calling.
    ToolUse,
    /// Code generation optimized.
    Coding,
    /// >100K token context.
    LongContext,
    /// Low-latency inference.
    Fast,
    /// Cost-optimized.
    Cheap,
    /// Text/content generation.
    Writing,
}

impl ModelCapability {
    /// All capability variants, for iteration in UI.
    pub const ALL: &[ModelCapability] = &[
        Self::Thinking,
        Self::Vision,
        Self::ToolUse,
        Self::Coding,
        Self::LongContext,
        Self::Fast,
        Self::Cheap,
        Self::Writing,
    ];
}

impl std::str::FromStr for ModelCapability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "thinking" => Ok(Self::Thinking),
            "vision" => Ok(Self::Vision),
            "tool_use" => Ok(Self::ToolUse),
            "coding" => Ok(Self::Coding),
            "long_context" => Ok(Self::LongContext),
            "fast" => Ok(Self::Fast),
            "cheap" => Ok(Self::Cheap),
            "writing" => Ok(Self::Writing),
            other => Err(format!("unknown capability: {other}")),
        }
    }
}

impl std::fmt::Display for ModelCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Thinking => write!(f, "thinking"),
            Self::Vision => write!(f, "vision"),
            Self::ToolUse => write!(f, "tool_use"),
            Self::Coding => write!(f, "coding"),
            Self::LongContext => write!(f, "long_context"),
            Self::Fast => write!(f, "fast"),
            Self::Cheap => write!(f, "cheap"),
            Self::Writing => write!(f, "writing"),
        }
    }
}

// default_capabilities moved to ozzie-cli::config_input::presets (wizard UX logic).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let cap = ModelCapability::ToolUse;
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, r#""tool_use""#);
        let parsed: ModelCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, cap);
    }

    #[test]
    fn all_variants_serialize() {
        let caps = vec![
            ModelCapability::Thinking,
            ModelCapability::Vision,
            ModelCapability::ToolUse,
            ModelCapability::Coding,
            ModelCapability::LongContext,
            ModelCapability::Fast,
            ModelCapability::Cheap,
            ModelCapability::Writing,
        ];
        for cap in caps {
            let json = serde_json::to_string(&cap).unwrap();
            let parsed: ModelCapability = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, cap);
        }
    }

    #[test]
    fn display_matches_serde() {
        assert_eq!(ModelCapability::LongContext.to_string(), "long_context");
        assert_eq!(ModelCapability::ToolUse.to_string(), "tool_use");
    }
}
