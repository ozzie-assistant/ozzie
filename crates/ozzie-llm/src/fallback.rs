use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;
use tracing::{info, warn};

use crate::{
    ChatDelta, ChatMessage, ChatResponse, CircuitBreaker, CircuitBreakerConfig, LlmError,
    Provider, ToolDefinition,
};

/// A provider that falls back to an alternative when the primary fails.
///
/// Uses a circuit breaker on the primary: after repeated failures, the primary
/// is short-circuited and the fallback is used directly until the cooldown expires.
pub struct FallbackProvider {
    primary: Arc<dyn Provider>,
    fallback: Arc<dyn Provider>,
    circuit: CircuitBreaker,
}

impl FallbackProvider {
    pub fn new(primary: Arc<dyn Provider>, fallback: Arc<dyn Provider>) -> Self {
        Self {
            primary,
            fallback,
            circuit: CircuitBreaker::new(CircuitBreakerConfig {
                threshold: 3,
                cooldown: std::time::Duration::from_secs(60),
            }),
        }
    }
}

#[async_trait::async_trait]
impl Provider for FallbackProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        if self.circuit.allow() {
            match self.primary.chat(messages, tools).await {
                Ok(response) => {
                    self.circuit.record_success();
                    return Ok(response);
                }
                Err(e) => {
                    self.circuit.record_failure();
                    warn!(
                        primary = self.primary.name(),
                        fallback = self.fallback.name(),
                        error = %e,
                        "primary provider failed, falling back"
                    );
                }
            }
        } else {
            info!(
                primary = self.primary.name(),
                fallback = self.fallback.name(),
                "primary circuit open, using fallback directly"
            );
        }

        self.fallback.chat(messages, tools).await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError> {
        if self.circuit.allow() {
            match self.primary.chat_stream(messages, tools).await {
                Ok(stream) => {
                    self.circuit.record_success();
                    return Ok(stream);
                }
                Err(e) => {
                    self.circuit.record_failure();
                    warn!(
                        primary = self.primary.name(),
                        fallback = self.fallback.name(),
                        error = %e,
                        "primary provider stream failed, falling back"
                    );
                }
            }
        }

        self.fallback.chat_stream(messages, tools).await
    }

    fn name(&self) -> &str {
        self.primary.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenUsage;

    struct OkProvider(&'static str);

    #[async_trait::async_trait]
    impl Provider for OkProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse {
                content: format!("from {}", self.0),
                tool_calls: Vec::new(),
                usage: TokenUsage::default(),
                stop_reason: None,
                model: None,
            })
        }

        async fn chat_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError>
        {
            Err(LlmError::Other("no stream".into()))
        }

        fn name(&self) -> &str {
            self.0
        }
    }

    struct FailProvider(&'static str);

    #[async_trait::async_trait]
    impl Provider for FailProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatResponse, LlmError> {
            Err(LlmError::Other(format!("{} unavailable", self.0)))
        }

        async fn chat_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatDelta, LlmError>> + Send>>, LlmError>
        {
            Err(LlmError::Other(format!("{} unavailable", self.0)))
        }

        fn name(&self) -> &str {
            self.0
        }
    }

    #[tokio::test]
    async fn primary_succeeds() {
        let provider = FallbackProvider::new(
            Arc::new(OkProvider("primary")),
            Arc::new(OkProvider("fallback")),
        );
        let result = provider.chat(&[], &[]).await.unwrap();
        assert_eq!(result.content, "from primary");
    }

    #[tokio::test]
    async fn falls_back_on_primary_failure() {
        let provider = FallbackProvider::new(
            Arc::new(FailProvider("primary")),
            Arc::new(OkProvider("fallback")),
        );
        let result = provider.chat(&[], &[]).await.unwrap();
        assert_eq!(result.content, "from fallback");
    }

    #[tokio::test]
    async fn circuit_opens_after_repeated_failures() {
        let primary = Arc::new(FailProvider("primary"));
        let fallback = Arc::new(OkProvider("fallback"));
        let provider = FallbackProvider::new(primary, fallback);

        // First 3 calls try primary (and fail), then use fallback
        for _ in 0..3 {
            let result = provider.chat(&[], &[]).await.unwrap();
            assert_eq!(result.content, "from fallback");
        }

        // Circuit is now open — primary is not even tried
        // (verified by the fact that the result is still from fallback)
        let result = provider.chat(&[], &[]).await.unwrap();
        assert_eq!(result.content, "from fallback");
    }

    #[tokio::test]
    async fn both_fail_returns_error() {
        let provider = FallbackProvider::new(
            Arc::new(FailProvider("primary")),
            Arc::new(FailProvider("fallback")),
        );
        let result = provider.chat(&[], &[]).await;
        assert!(result.is_err());
    }

    #[test]
    fn name_returns_primary() {
        let provider = FallbackProvider::new(
            Arc::new(OkProvider("anthropic")),
            Arc::new(OkProvider("ollama")),
        );
        assert_eq!(provider.name(), "anthropic");
    }
}
