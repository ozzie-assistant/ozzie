use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;

use crate::{ChatDelta, ChatMessage, ChatResponse, LlmError, Provider, ResolvedAuth, ToolDefinition};

use super::OpenAIProvider;

const DEFAULT_MODEL: &str = "grok-3-mini-fast";
const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";

/// Known xAI model context windows (in tokens).
const XAI_CONTEXT_WINDOWS: &[(&str, u32)] = &[
    ("grok-3-mini-fast", 128_000),
    ("grok-3-mini", 128_000),
    ("grok-4-fast", 128_000),
    ("grok-4", 128_000),
];

/// xAI (Grok) provider (OpenAI-compatible API).
///
/// Wraps [`OpenAIProvider`] with xAI-specific defaults:
/// - Default model: `grok-3-mini-fast`
/// - Default base URL: `https://api.x.ai/v1`
/// - Auth via `XAI_API_KEY`
pub struct XaiProvider {
    inner: OpenAIProvider,
}

impl XaiProvider {
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
                Some("xai"),
            ),
        }
    }

    /// Returns the context window size for a known xAI model, or `None`.
    pub fn context_window(model: &str) -> Option<u32> {
        XAI_CONTEXT_WINDOWS
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
impl Provider for XaiProvider {
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
        "xai"
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
        assert_eq!(XaiProvider::default_model(), "grok-3-mini-fast");
    }

    #[test]
    fn default_base_url_value() {
        assert_eq!(
            XaiProvider::default_base_url(),
            "https://api.x.ai/v1"
        );
    }

    #[test]
    fn provider_name_is_xai() {
        let provider = XaiProvider::new(test_auth(), None, None, None, None);
        assert_eq!(provider.name(), "xai");
    }

    #[test]
    fn context_window_known_models() {
        assert_eq!(XaiProvider::context_window("grok-3-mini-fast"), Some(128_000));
        assert_eq!(XaiProvider::context_window("grok-3-mini"), Some(128_000));
        assert_eq!(XaiProvider::context_window("grok-4-fast"), Some(128_000));
        assert_eq!(XaiProvider::context_window("grok-4"), Some(128_000));
    }

    #[test]
    fn context_window_unknown_model() {
        assert_eq!(XaiProvider::context_window("unknown-model"), None);
    }

    #[test]
    fn custom_model_and_base_url() {
        let provider = XaiProvider::new(
            test_auth(),
            Some("grok-4"),
            Some("https://custom.x.ai/v1"),
            Some(8192),
            None,
        );
        assert_eq!(provider.name(), "xai");
    }

    #[test]
    fn auth_resolution_xai_driver() {
        let auth = crate::resolve_auth(crate::Driver::Xai, Some("xai-test"), None, &crate::EnvSecretResolver).unwrap();
        assert_eq!(auth.kind, AuthKind::ApiKey);
        assert_eq!(auth.value, "xai-test");
    }
}
