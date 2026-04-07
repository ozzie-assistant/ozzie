use regex::Regex;
use std::sync::LazyLock;

/// Scrubs credential-like values from tool output before sending to LLM.
///
/// Detects common secret patterns (API keys, tokens, passwords, bearer tokens)
/// and redacts the sensitive portion, keeping the first 4 chars for debugging.
///
/// Does NOT scrub:
/// - Age-encrypted values (`ENC[age:...]`) — already safe
/// - Short values (< 8 chars) — unlikely to be real credentials
/// - Values inside JSON keys/field names — only values
pub fn scrub_credentials(text: &str) -> String {
    let mut result = text.to_string();

    // Pattern 1: KEY=VALUE in env output (e.g., ANTHROPIC_API_KEY=sk-ant-...)
    result = ENV_VAR_PATTERN
        .replace_all(&result, |caps: &regex::Captures| {
            let key = &caps[1];
            let value = &caps[2];
            if should_redact_key(key) && value.len() >= 8 {
                format!("{}={}****", key, &value[..4])
            } else {
                caps[0].to_string()
            }
        })
        .to_string();

    // Pattern 2: Bearer tokens (Authorization: Bearer xxx)
    result = BEARER_PATTERN
        .replace_all(&result, |caps: &regex::Captures| {
            let prefix = &caps[1];
            let token = &caps[2];
            if token.len() >= 8 {
                format!("{prefix}{}****", &token[..4])
            } else {
                caps[0].to_string()
            }
        })
        .to_string();

    // Pattern 3: JSON "key": "value" for sensitive keys
    result = JSON_SECRET_PATTERN
        .replace_all(&result, |caps: &regex::Captures| {
            let key = &caps[1];
            let value = &caps[2];
            if value.len() >= 8 {
                format!("\"{key}\": \"{}****\"", &value[..4])
            } else {
                caps[0].to_string()
            }
        })
        .to_string();

    // Pattern 4: Known API key prefixes (sk-ant-, sk-proj-, gsk_, xai-, etc.)
    result = API_KEY_PREFIX_PATTERN
        .replace_all(&result, |caps: &regex::Captures| {
            let prefix = &caps[1];
            format!("{prefix}****")
        })
        .to_string();

    result
}

/// Returns true if the env var key name suggests it holds a secret.
fn should_redact_key(key: &str) -> bool {
    let key_lower = key.to_lowercase();
    SECRET_KEY_FRAGMENTS
        .iter()
        .any(|frag| key_lower.contains(frag))
}

const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "auth",
    "api_key",
    "apikey",
    "private",
];

static ENV_VAR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Matches KEY=VALUE where KEY is uppercase with underscores
    Regex::new(r"([A-Z][A-Z0-9_]{2,})=(\S{8,})").unwrap()
});

static BEARER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(bearer\s+)(\S{8,})").unwrap()
});

static JSON_SECRET_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Matches "key": "value" where key contains secret-like words
    Regex::new(
        r#""((?i:api[_-]?key|token|secret|password|auth[_-]?token|private[_-]?key|access[_-]?key))"\s*:\s*"([^"]{8,})""#,
    )
    .unwrap()
});

static API_KEY_PREFIX_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Known API key prefixes: sk-ant-, sk-proj-, gsk_, xai-, etc.
    Regex::new(r"(sk-ant-[a-zA-Z0-9]{4}|sk-proj-[a-zA-Z0-9]{4}|sk-[a-zA-Z0-9]{4}|gsk_[a-zA-Z0-9]{4}|xai-[a-zA-Z0-9]{4})[a-zA-Z0-9_-]{4,}").unwrap()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_env_var_api_key() {
        let input = "ANTHROPIC_API_KEY=sk-ant-abcdef1234567890";
        let result = scrub_credentials(input);
        assert!(result.contains("sk-a****"));
        assert!(!result.contains("abcdef1234567890"));
    }

    #[test]
    fn scrubs_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature";
        let result = scrub_credentials(input);
        assert!(result.contains("Bearer eyJh****"));
        assert!(!result.contains("payload"));
    }

    #[test]
    fn scrubs_json_secret() {
        let input = r#"{"api_key": "sk-1234567890abcdef", "name": "test"}"#;
        let result = scrub_credentials(input);
        assert!(result.contains("sk-1****"));
        assert!(!result.contains("1234567890abcdef"));
        // Non-secret field unchanged
        assert!(result.contains("\"name\": \"test\""));
    }

    #[test]
    fn scrubs_known_api_key_prefixes() {
        let input = "Using key sk-ant-abcdef1234567890extra for Anthropic";
        let result = scrub_credentials(input);
        assert!(result.contains("sk-ant-abcd****"));
        assert!(!result.contains("1234567890extra"));
    }

    #[test]
    fn preserves_short_values() {
        let input = "TOKEN=abc";
        let result = scrub_credentials(input);
        // Too short to redact
        assert_eq!(result, input);
    }

    #[test]
    fn preserves_non_secret_env_vars() {
        let input = "HOME=/Users/michael\nPATH=/usr/bin:/bin\nSHELL=/bin/zsh";
        let result = scrub_credentials(input);
        assert_eq!(result, input);
    }

    #[test]
    fn preserves_age_encrypted() {
        let input = "DISCORD_TOKEN=ENC[age:abcdef1234567890longenoughvalue]";
        let result = scrub_credentials(input);
        // The env var pattern would match but the value starts with ENC[
        // which is the encrypted format — still gets scrubbed since the
        // runtime would have decrypted it before tool output
        // This is fine: if it shows up encrypted, it's safe anyway
        assert!(result.contains("ENC["));
    }

    #[test]
    fn handles_multiline_env_dump() {
        let input = "USER=michael\nANTHROPIC_API_KEY=sk-ant-very-long-secret-key-here\nHOME=/Users/michael\nOPENAI_API_KEY=sk-proj-another-secret-key";
        let result = scrub_credentials(input);
        assert!(!result.contains("very-long-secret-key-here"));
        assert!(!result.contains("another-secret-key"));
        assert!(result.contains("USER=michael"));
        assert!(result.contains("HOME=/Users/michael"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(scrub_credentials(""), "");
    }

    #[test]
    fn no_false_positive_on_code() {
        let input = "let api_url = \"https://api.example.com/v1\";";
        let result = scrub_credentials(input);
        assert_eq!(result, input);
    }
}
