// Re-export prompt types from ozzie-types for backward compatibility.
// These types are the canonical source in ozzie-types; this module
// re-exports them so existing code that imports from ozzie-protocol
// continues to work during migration.

pub use ozzie_types::{PromptOption, PromptResponseParams};

/// Prompt request payload for client-side deserialization.
/// Re-exports `PromptRequestEvent` from ozzie-types with an alias.
pub type PromptRequestPayload = ozzie_types::PromptRequestEvent;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_response_params_roundtrip() {
        let params = PromptResponseParams {
            token: "tok_123".to_string(),
            value: Some("session".to_string()),
            text: None,
        };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["token"], "tok_123");
        assert_eq!(json["value"], "session");
        assert!(json.get("text").is_none());

        let parsed: PromptResponseParams = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.token, "tok_123");
        assert_eq!(parsed.value.as_deref(), Some("session"));
        assert!(parsed.text.is_none());
    }
}
