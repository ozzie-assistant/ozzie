use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;

use crate::{ChatDelta, ChatMessage, ChatResponse, LlmError, Provider, ResolvedAuth, ToolDefinition};

use super::OpenAIProvider;

const DEFAULT_MODEL: &str = "qwen/qwen3-32b";
const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Known Groq model context windows (in tokens).
const GROQ_CONTEXT_WINDOWS: &[(&str, u32)] = &[
    ("qwen/qwen3-32b", 32_768),
    ("llama-3.3-70b-versatile", 128_000),
    ("llama-3.1-8b-instant", 128_000),
    ("deepseek-r1-distill-llama-70b", 128_000),
    ("mixtral-8x7b-32768", 32_768),
    ("gemma2-9b-it", 8_192),
];

/// Groq provider (OpenAI-compatible API).
///
/// Wraps [`OpenAIProvider`] with Groq-specific defaults:
/// - Default model: `llama-3.3-70b-versatile`
/// - Default base URL: `https://api.groq.com/openai/v1`
/// - Auth via `GROQ_API_KEY`
pub struct GroqProvider {
    inner: OpenAIProvider,
}

impl GroqProvider {
    pub fn new(
        auth: ResolvedAuth,
        model: Option<&str>,
        base_url: Option<&str>,
        max_tokens: Option<u32>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            inner: OpenAIProvider::new(
                auth,
                Some(model.unwrap_or(DEFAULT_MODEL)),
                Some(base_url.unwrap_or(DEFAULT_BASE_URL)),
                max_tokens,
                timeout,
                Some("groq"),
            ),
        }
    }

    /// Returns the context window size for a known Groq model, or `None`.
    pub fn context_window(model: &str) -> Option<u32> {
        GROQ_CONTEXT_WINDOWS
            .iter()
            .find(|(name, _)| model == *name)
            .map(|(_, size)| *size)
    }

    /// Returns the default model name.
    pub fn default_model() -> &'static str {
        DEFAULT_MODEL
    }

    /// Returns the default base URL.
    pub fn default_base_url() -> &'static str {
        DEFAULT_BASE_URL
    }
}

#[async_trait::async_trait]
impl Provider for GroqProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        self.inner.chat(messages, tools).await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        self.inner.chat_stream(messages, tools).await
    }

    fn name(&self) -> &str {
        "groq"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthKind, ResolvedAuth};

    fn test_auth() -> ResolvedAuth {
        ResolvedAuth {
            kind: AuthKind::ApiKey,
            value: "test-key".to_string(),
        }
    }

    #[test]
    fn default_model_name() {
        assert_eq!(GroqProvider::default_model(), "qwen/qwen3-32b");
    }

    #[test]
    fn default_base_url_value() {
        assert_eq!(
            GroqProvider::default_base_url(),
            "https://api.groq.com/openai/v1"
        );
    }

    #[test]
    fn provider_name_is_groq() {
        let provider = GroqProvider::new(test_auth(), None, None, None, None);
        assert_eq!(provider.name(), "groq");
    }

    #[test]
    fn context_window_known_models() {
        assert_eq!(GroqProvider::context_window("llama-3.3-70b-versatile"), Some(128_000));
        assert_eq!(GroqProvider::context_window("llama-3.1-8b-instant"), Some(128_000));
        assert_eq!(GroqProvider::context_window("mixtral-8x7b-32768"), Some(32_768));
    }

    #[test]
    fn context_window_unknown_model() {
        assert_eq!(GroqProvider::context_window("unknown-model"), None);
    }

    #[test]
    fn custom_model_and_base_url() {
        let provider = GroqProvider::new(
            test_auth(),
            Some("llama-3.1-8b-instant"),
            Some("https://custom.groq.com/v1"),
            Some(8192),
            None,
        );
        assert_eq!(provider.name(), "groq");
    }

    #[test]
    fn auth_resolution_groq_driver() {
        let auth = crate::resolve_auth(ozzie_core::config::Driver::Groq, Some("gsk_test"), None).unwrap();
        assert_eq!(auth.kind, AuthKind::ApiKey);
        assert_eq!(auth.value, "gsk_test");
    }
}
