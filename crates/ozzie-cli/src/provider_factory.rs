use std::sync::Arc;

use ozzie_core::config::{self, Driver};
use ozzie_utils::secrets::SecretStore;

/// Adapter: implements `ozzie_llm::SecretResolver` for Ozzie's `SecretStore`.
pub struct OzzieSecretResolver;

impl ozzie_llm::SecretResolver for OzzieSecretResolver {
    fn get(&self, key: &str) -> Option<String> {
        SecretStore::global().get(key)
    }

    fn resolve_value(&self, value: &str) -> String {
        SecretStore::global().resolve_value(value)
    }
}

/// Builds an LLM provider from a named provider configuration entry.
///
/// Used by both the gateway (to initialize the full registry) and the
/// wake wizard (to make a single onboarding LLM call).
pub fn build_provider(
    name: &str,
    provider_cfg: &config::ProviderConfig,
    secret_resolver: &dyn ozzie_llm::SecretResolver,
) -> anyhow::Result<Arc<dyn ozzie_llm::Provider>> {
    let driver = provider_cfg.driver;
    let auth = ozzie_llm::resolve_auth(
        driver,
        provider_cfg.auth.api_key.as_deref(),
        provider_cfg.auth.token.as_deref(),
        secret_resolver,
    )
    .map_err(|e| anyhow::anyhow!("auth resolution for '{name}' failed: {e}"))?;

    let model = &provider_cfg.model;
    let base_url = provider_cfg.base_url.as_deref();
    let max_tokens = provider_cfg.max_tokens;
    let timeout = provider_cfg.timeout;

    let provider: Arc<dyn ozzie_llm::Provider> = match driver {
        Driver::Anthropic => Arc::new(ozzie_llm::providers::AnthropicProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
        )),
        Driver::OpenAi => Arc::new(ozzie_llm::providers::OpenAIProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
            None,
        )),
        Driver::Gemini => Arc::new(ozzie_llm::providers::GeminiProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
        )),
        Driver::Mistral => Arc::new(ozzie_llm::providers::MistralProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
        )),
        Driver::Groq => Arc::new(ozzie_llm::providers::GroqProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
        )),
        Driver::Xai => Arc::new(ozzie_llm::providers::XaiProvider::new(
            auth,
            Some(model),
            base_url,
            max_tokens,
            timeout,
        )),
        Driver::Ollama => {
            let native_tools = provider_cfg
                .capabilities
                .contains(&ozzie_core::domain::ModelCapability::ToolUse);
            Arc::new(ozzie_llm::providers::OllamaProvider::with_native_tools(
                model, base_url, timeout, native_tools,
            ))
        }
        Driver::OpenAiCompatible | Driver::LmStudio | Driver::Vllm => {
            let native_tools = provider_cfg
                .capabilities
                .contains(&ozzie_core::domain::ModelCapability::ToolUse);
            Arc::new(ozzie_llm::providers::OpenAIProvider::with_native_tools(
                auth,
                Some(model),
                base_url,
                max_tokens,
                timeout,
                Some(driver.as_str()),
                native_tools,
            ))
        }
    };

    Ok(provider)
}
