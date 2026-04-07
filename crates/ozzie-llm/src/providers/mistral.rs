use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;

use crate::{ChatDelta, ChatMessage, ChatResponse, LlmError, Provider, ResolvedAuth, ToolDefinition};

use super::OpenAIProvider;

const DEFAULT_MODEL: &str = "mistral-small-latest";
const DEFAULT_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Known Mistral model context windows (in tokens).
const MISTRAL_CONTEXT_WINDOWS: &[(&str, u32)] = &[
    ("mistral-large", 128_000),
    ("mistral-small", 128_000),
    ("codestral", 256_000),
    ("open-mistral-nemo", 128_000),
    ("pixtral", 128_000),
];

/// Mistral AI provider (OpenAI-compatible API).
///
/// Wraps [`OpenAIProvider`] with Mistral-specific defaults:
/// - Default model: `mistral-small-latest`
/// - Default base URL: `https://api.mistral.ai/v1`
/// - Auth via `MISTRAL_API_KEY`
pub struct MistralProvider {
    inner: OpenAIProvider,
}

impl MistralProvider {
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
                Some("mistral"),
            ),
        }
    }

    /// Returns the context window size for a known Mistral model, or `None`.
    pub fn context_window(model: &str) -> Option<u32> {
        // Strip version suffixes like "-latest", "-2024-09-xx"
        let base = strip_version_suffix(model);
        MISTRAL_CONTEXT_WINDOWS
            .iter()
            .find(|(name, _)| base == *name || model == *name)
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

/// Strips common version suffixes from Mistral model names.
/// e.g. "mistral-small-latest" -> "mistral-small"
/// e.g. "mistral-large-2411" -> "mistral-large"
fn strip_version_suffix(model: &str) -> &str {
    if let Some(base) = model.strip_suffix("-latest") {
        return base;
    }
    // Strip date suffixes like "-2411", "-2024-09-12"
    if let Some(pos) = model.rfind('-') {
        let suffix = &model[pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return &model[..pos];
        }
    }
    model
}

#[async_trait::async_trait]
impl Provider for MistralProvider {
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
        "mistral"
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
        assert_eq!(MistralProvider::default_model(), "mistral-small-latest");
    }

    #[test]
    fn default_base_url_value() {
        assert_eq!(
            MistralProvider::default_base_url(),
            "https://api.mistral.ai/v1"
        );
    }

    #[test]
    fn provider_name_is_mistral() {
        let provider = MistralProvider::new(test_auth(), None, None, None, None);
        assert_eq!(provider.name(), "mistral");
    }

    #[test]
    fn context_window_known_models() {
        assert_eq!(MistralProvider::context_window("mistral-large"), Some(128_000));
        assert_eq!(MistralProvider::context_window("mistral-small"), Some(128_000));
        assert_eq!(MistralProvider::context_window("codestral"), Some(256_000));
        assert_eq!(MistralProvider::context_window("open-mistral-nemo"), Some(128_000));
        assert_eq!(MistralProvider::context_window("pixtral"), Some(128_000));
    }

    #[test]
    fn context_window_with_version_suffix() {
        assert_eq!(
            MistralProvider::context_window("mistral-small-latest"),
            Some(128_000)
        );
        assert_eq!(
            MistralProvider::context_window("mistral-large-latest"),
            Some(128_000)
        );
        assert_eq!(
            MistralProvider::context_window("mistral-large-2411"),
            Some(128_000)
        );
    }

    #[test]
    fn context_window_unknown_model() {
        assert_eq!(MistralProvider::context_window("unknown-model"), None);
        assert_eq!(MistralProvider::context_window("gpt-4o"), None);
    }

    #[test]
    fn custom_model_and_base_url() {
        let provider = MistralProvider::new(
            test_auth(),
            Some("codestral-latest"),
            Some("https://custom.mistral.ai/v1"),
            Some(8192),
            None,
        );
        assert_eq!(provider.name(), "mistral");
    }

    #[test]
    fn strip_version_suffix_cases() {
        assert_eq!(strip_version_suffix("mistral-small-latest"), "mistral-small");
        assert_eq!(strip_version_suffix("mistral-large-2411"), "mistral-large");
        assert_eq!(strip_version_suffix("codestral"), "codestral");
        assert_eq!(strip_version_suffix("open-mistral-nemo"), "open-mistral-nemo");
        assert_eq!(strip_version_suffix("pixtral"), "pixtral");
    }

    #[test]
    fn auth_resolution_mistral_driver() {
        let auth = crate::resolve_auth(ozzie_core::config::Driver::Mistral, Some("my-key"), None).unwrap();
        assert_eq!(auth.kind, AuthKind::ApiKey);
        assert_eq!(auth.value, "my-key");
    }
}
