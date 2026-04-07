use std::sync::Arc;

use ozzie_core::layered::SummarizerFn;
use ozzie_llm::Provider;

const SUMMARIZE_PROMPT_TEMPLATE: &str = r#"Summarize the following conversation excerpt concisely.
Preserve: key decisions, preferences, unresolved tasks, technical details, and names.
Discard: greetings, filler, repetition.
Target length: approximately {target} tokens.
Output ONLY the summary, no preamble.

---
{text}
---"#;

/// Creates an LLM-backed summarizer function for the layered context system.
///
/// Uses `block_on` internally because the indexer pipeline is synchronous.
/// This is safe because the indexer runs inside a `tokio::spawn` context
/// where `block_on` on a new runtime is acceptable.
pub fn llm_summarizer(provider: Arc<dyn Provider>) -> SummarizerFn {
    Box::new(move |text: &str, target_tokens: usize| {
        // Skip LLM for very short texts — not worth the call
        if text.len() < 200 {
            return ozzie_core::layered::fallback_summarizer(text, target_tokens);
        }

        let prompt = SUMMARIZE_PROMPT_TEMPLATE
            .replace("{target}", &target_tokens.to_string())
            .replace("{text}", text);

        let provider = provider.clone();

        // Use a separate tokio runtime for the blocking LLM call.
        // We can't use Handle::block_on because the indexer may be called
        // from within a tokio context where blocking is not allowed.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(_) => {
                tracing::warn!("failed to create runtime for LLM summarizer, using fallback");
                return ozzie_core::layered::fallback_summarizer(text, target_tokens);
            }
        };

        match rt.block_on(call_llm(&provider, &prompt)) {
            Ok(summary) => {
                let trimmed = ozzie_core::layered::trim_to_tokens(&summary, target_tokens);
                trimmed.to_string()
            }
            Err(e) => {
                tracing::warn!(error = %e, "LLM summarizer failed, using fallback");
                ozzie_core::layered::fallback_summarizer(text, target_tokens)
            }
        }
    })
}

async fn call_llm(
    provider: &Arc<dyn Provider>,
    prompt: &str,
) -> Result<String, ozzie_llm::LlmError> {
    let messages = vec![ozzie_llm::ChatMessage {
        role: ozzie_llm::ChatRole::User,
        content: prompt.to_string(),
        tool_calls: Vec::new(),
        tool_call_id: None,
    }];

    let response = provider.chat(&messages, &[]).await?;
    Ok(response.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_uses_fallback() {
        // Mock provider — won't be called for short text
        let provider = Arc::new(MockProvider);
        let summarizer = llm_summarizer(provider);

        let result = summarizer("short text", 100);
        // Should use fallback (text < 200 chars)
        assert!(!result.is_empty());
    }

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

    #[test]
    fn long_text_calls_llm() {
        let provider = Arc::new(MockProvider);
        let summarizer = llm_summarizer(provider);

        let long_text = "This is a detailed conversation about Rust programming. ".repeat(20);
        let result = summarizer(&long_text, 200);
        assert_eq!(result, "Mock summary of the conversation.");
    }
}
