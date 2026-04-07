use std::time::Duration;

/// Errors from LLM providers.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("rate limited: {message}")]
    RateLimit {
        message: String,
        /// Hint from the server on when to retry.
        retry_after: Option<Duration>,
    },

    #[error("context too long: {0}")]
    ContextTooLong(String),

    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("model unavailable ({provider}): {body}")]
    ModelUnavailable { provider: String, body: String },

    #[error("circuit breaker open")]
    CircuitOpen,

    #[error("{0}")]
    Other(String),
}

impl LlmError {
    /// Classifies a raw error string into a typed error.
    ///
    /// Known API key patterns are scrubbed from the message before storage.
    pub fn classify(err: &str) -> Self {
        let lower = err.to_lowercase();
        let safe = scrub_secrets(err);

        if contains_any(&lower, &["401", "403", "unauthorized", "invalid api key", "forbidden"]) {
            return Self::Auth(safe);
        }
        if contains_any(&lower, &["429", "rate limit", "quota", "too many requests"]) {
            let retry_after = parse_retry_after(err);
            return Self::RateLimit {
                message: safe,
                retry_after,
            };
        }
        if contains_any(&lower, &["context length", "too many tokens", "max tokens", "token limit"]) {
            return Self::ContextTooLong(safe);
        }
        if contains_any(&lower, &["model not found", "404", "not found"]) {
            return Self::ModelNotFound(safe);
        }
        if contains_any(&lower, &["connection", "eof", "timeout", "dial", "refused"]) {
            return Self::Connection(safe);
        }

        Self::Other(safe)
    }

    /// Returns true if the error is transient and should be retried.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimit { .. } | Self::Connection(_) | Self::ModelUnavailable { .. }
        )
    }

    /// Returns true if this is a rate limit error (longer backoff).
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }
}

/// Attempts to extract a Retry-After value (in seconds) from an error string.
///
/// Looks for patterns like `retry-after: 30` or `retry after 5 seconds`.
fn parse_retry_after(err: &str) -> Option<Duration> {
    let lower = err.to_lowercase();
    // Pattern: "retry-after: <N>" or "retry-after:<N>"
    if let Some(pos) = lower.find("retry-after") {
        let rest = &lower[pos + 11..];
        let rest = rest.trim_start_matches([':',  ' ']);
        if let Some(Ok(n)) = rest
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .map(|s| s.parse::<u64>())
            && n > 0
            && n < 3600
        {
            return Some(Duration::from_secs(n));
        }
    }
    None
}

/// Known API key prefixes to scrub from error messages.
/// Ordered longest-first so specific prefixes match before generic ones.
const SECRET_PREFIXES: &[&str] = &[
    "sk-ant-api03-", // Anthropic (most specific)
    "sk-ant-oat01-", // Anthropic OAuth
    "sk-ant-",       // Anthropic (short)
    "sk-proj-",      // OpenAI project keys
    "nvapi-",        // NVIDIA
    "pplx-",         // Perplexity
    "gsk_",          // Groq
    "xai-",          // xAI
    "KEY-",          // generic
    "sk-",           // OpenAI (shortest, must be last among sk- variants)
];

/// Redacts known API key patterns from a string.
///
/// Replaces the token value with `<REDACTED>`, keeping the prefix for diagnostics.
/// Example: `"key sk-abc123def456 is invalid"` → `"key sk-<REDACTED> is invalid"`
pub fn scrub_secrets(s: &str) -> String {
    let mut result = s.to_string();
    // Track byte ranges already redacted to prevent shorter prefixes
    // from re-matching inside a longer prefix's redacted region.
    let mut redacted_ranges: Vec<(usize, usize)> = Vec::new();

    for prefix in SECRET_PREFIXES {
        let mut search_from = 0;
        while let Some(rel) = result[search_from..].find(prefix) {
            let start = search_from + rel;
            let after = start + prefix.len();

            // Skip if this position overlaps with an already-redacted range
            if redacted_ranges.iter().any(|&(rs, re)| start >= rs && start < re) {
                search_from = after;
                continue;
            }

            // Find the end of the token (next delimiter or end of string)
            let end = result[after..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == '}')
                .map_or(result.len(), |pos| after + pos);

            if end > after {
                result.replace_range(after..end, "<REDACTED>");
                let new_end = after + "<REDACTED>".len();
                redacted_ranges.push((start, new_end));
                search_from = new_end;
            } else {
                search_from = after;
            }
        }
    }
    result
}

fn contains_any(s: &str, substrs: &[&str]) -> bool {
    substrs.iter().any(|sub| s.contains(sub))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_auth() {
        let e = LlmError::classify("401 unauthorized");
        assert!(matches!(e, LlmError::Auth(_)));
        assert!(!e.is_retryable());
    }

    #[test]
    fn classify_rate_limit() {
        let e = LlmError::classify("429 rate limit exceeded");
        assert!(matches!(e, LlmError::RateLimit { .. }));
        assert!(e.is_retryable());
        assert!(e.is_rate_limit());
    }

    #[test]
    fn classify_rate_limit_with_retry_after() {
        let e = LlmError::classify("429 rate limit exceeded, retry-after: 30");
        match &e {
            LlmError::RateLimit { retry_after, .. } => {
                assert_eq!(*retry_after, Some(Duration::from_secs(30)));
            }
            _ => panic!("expected RateLimit"),
        }
    }

    #[test]
    fn classify_connection() {
        let e = LlmError::classify("connection refused");
        assert!(matches!(e, LlmError::Connection(_)));
        assert!(e.is_retryable());
    }

    #[test]
    fn classify_unknown() {
        let e = LlmError::classify("something went wrong");
        assert!(matches!(e, LlmError::Other(_)));
        assert!(!e.is_retryable());
    }

    #[test]
    fn model_unavailable_is_retryable() {
        let e = LlmError::ModelUnavailable {
            provider: "test".to_string(),
            body: "down".to_string(),
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn scrub_secrets_redacts_known_prefixes() {
        assert_eq!(
            scrub_secrets("invalid key sk-abc123def456 for openai"),
            "invalid key sk-<REDACTED> for openai"
        );
        assert_eq!(
            scrub_secrets("key sk-ant-api03-longtoken123 is expired"),
            "key sk-ant-api03-<REDACTED> is expired"
        );
        assert_eq!(
            scrub_secrets("gsk_mygroqkey failed"),
            "gsk_<REDACTED> failed"
        );
    }

    #[test]
    fn scrub_secrets_leaves_clean_strings() {
        assert_eq!(scrub_secrets("no secrets here"), "no secrets here");
        assert_eq!(scrub_secrets(""), "");
    }

    #[test]
    fn classify_scrubs_secrets() {
        let e = LlmError::classify("401 unauthorized: key sk-abc123 is invalid");
        let msg = e.to_string();
        assert!(!msg.contains("abc123"), "secret leaked: {msg}");
        assert!(msg.contains("sk-<REDACTED>"), "prefix missing: {msg}");
    }

    #[test]
    fn parse_retry_after_header() {
        assert_eq!(parse_retry_after("retry-after: 5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("Retry-After:30"), Some(Duration::from_secs(30)));
        assert_eq!(parse_retry_after("no hint here"), None);
        // Absurd values are rejected
        assert_eq!(parse_retry_after("retry-after: 0"), None);
        assert_eq!(parse_retry_after("retry-after: 99999"), None);
    }
}
