use std::sync::Arc;

use ozzie_core::layered::{fallback_summarize, Summarizer, SummarizerError};
use ozzie_llm::Provider;

const SUMMARIZE_PROMPT_TEMPLATE: &str = r#"Summarize the following conversation excerpt concisely.
Preserve: key decisions, preferences, unresolved tasks, technical details, and names.
Discard: greetings, filler, repetition.
Target length: approximately {target} tokens.
Output ONLY the summary, no preamble.

---
{text}
---"#;

/// LLM-backed summarizer for the layered context system.
///
/// Falls back to the heuristic summarizer for very short texts (< 200 chars)
/// or on LLM failure.
pub struct LlmSummarizer {
    provider: Arc<dyn Provider>,
}

impl LlmSummarizer {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl Summarizer for LlmSummarizer {
    async fn summarize(&self, text: &str, target_tokens: usize) -> Result<String, SummarizerError> {
        // Skip LLM for very short texts — not worth the call
        if text.len() < 200 {
            return Ok(fallback_summarize(text, target_tokens));
        }

        let prompt = SUMMARIZE_PROMPT_TEMPLATE
            .replace("{target}", &target_tokens.to_string())
            .replace("{text}", text);

        let messages =
            vec![ozzie_llm::ChatMessage::text(ozzie_llm::ChatRole::User, &prompt)];

        match self.provider.chat(&messages, &[]).await {
            Ok(response) => {
                let trimmed =
                    ozzie_core::layered::trim_to_tokens(&response.content, target_tokens);
                Ok(trimmed.to_string())
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM summarizer failed, using fallback");
                Ok(fallback_summarize(text, target_tokens))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider;

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn chat(
            &self,
            _messages: &[ozzie_llm::ChatMessage],
            _tools: &[ozzie_llm::ToolDefinition],
        ) -> Result<ozzie_llm::ChatResponse, ozzie_llm::LlmError> {
            Ok(ozzie_llm::ChatResponse {
                content: "Mock summary of the conversation.".to_string(),
                tool_calls: Vec::new(),
                usage: ozzie_llm::TokenUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            })
        }

        async fn chat_stream(
            &self,
            _messages: &[ozzie_llm::ChatMessage],
            _tools: &[ozzie_llm::ToolDefinition],
        ) -> Result<
            std::pin::Pin<
                Box<
                    dyn futures_core::Stream<
                            Item = Result<ozzie_llm::ChatDelta, ozzie_llm::LlmError>,
                        > + Send,
                >,
            >,
            ozzie_llm::LlmError,
        > {
            Err(ozzie_llm::LlmError::Other("no stream".to_string()))
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn short_text_uses_fallback() {
        let summarizer = LlmSummarizer::new(Arc::new(MockProvider));
        let result = summarizer.summarize("short text", 100).await.unwrap();
        // Should use fallback (text < 200 chars)
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn long_text_calls_llm() {
        let summarizer = LlmSummarizer::new(Arc::new(MockProvider));
        let long_text = "This is a detailed conversation about Rust programming. ".repeat(20);
        let result = summarizer.summarize(&long_text, 200).await.unwrap();
        assert_eq!(result, "Mock summary of the conversation.");
    }
}
