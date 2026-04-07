use std::collections::HashMap;
use std::sync::Arc;

use ozzie_llm::Provider;

use crate::react::TurnBudget;

/// Registry of named LLM providers with per-provider turn budgets.
///
/// Used by the gateway to hold all configured providers and let
/// subtask/schedule runners pick by name with the right budget.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
    budgets: HashMap<String, TurnBudget>,
    default_name: String,
}

impl ProviderRegistry {
    pub fn new(default_name: String) -> Self {
        Self {
            providers: HashMap::new(),
            budgets: HashMap::new(),
            default_name,
        }
    }

    pub fn register(&mut self, name: String, provider: Arc<dyn Provider>) {
        self.providers.insert(name, provider);
    }

    /// Registers a per-provider turn budget override.
    pub fn set_budget(&mut self, name: String, budget: TurnBudget) {
        self.budgets.insert(name, budget);
    }

    pub fn default_name(&self) -> &str {
        &self.default_name
    }

    pub fn default_provider(&self) -> &Arc<dyn Provider> {
        self.providers
            .get(&self.default_name)
            .expect("default provider must be registered")
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.get(name)
    }

    /// Returns the turn budget for a provider, or the given default if none configured.
    pub fn budget_for(&self, name: &str, base: TurnBudget) -> TurnBudget {
        self.budgets.get(name).cloned().unwrap_or(base)
    }

    pub fn names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_llm::{ChatDelta, ChatMessage, ChatResponse, LlmError, TokenUsage, ToolDefinition};

    struct DummyProvider(&'static str);

    #[async_trait::async_trait]
    impl Provider for DummyProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatResponse, LlmError> {
            Ok(ChatResponse {
                content: String::new(),
                tool_calls: Vec::new(),
                usage: TokenUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            })
        }

        async fn chat_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<
            std::pin::Pin<
                Box<dyn futures_core::Stream<Item = Result<ChatDelta, LlmError>> + Send>,
            >,
            LlmError,
        > {
            Err(LlmError::Other("no stream".to_string()))
        }

        fn name(&self) -> &str {
            self.0
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = ProviderRegistry::new("anthropic".to_string());
        reg.register("anthropic".to_string(), Arc::new(DummyProvider("anthropic")));
        reg.register("ollama".to_string(), Arc::new(DummyProvider("ollama")));

        assert_eq!(reg.default_provider().name(), "anthropic");
        assert_eq!(reg.get("ollama").unwrap().name(), "ollama");
        assert!(reg.get("unknown").is_none());
        assert_eq!(reg.names().len(), 2);
    }
}
