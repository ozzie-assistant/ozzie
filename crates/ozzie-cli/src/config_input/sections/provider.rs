use std::collections::HashMap;

use ozzie_core::config::{AuthConfig, Driver, ModelsConfig, ProviderConfig};
use ozzie_core::domain::ModelCapability;

use super::super::section::{
    confirm_add_more, BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue,
    InputCollector, SelectOption,
};

// ── Field keys ─────────────────────────────────────────────────────────────

enum Field {
    Driver,
    Alias,
    Model,
    BaseUrl,
    ApiKey,
    AddMore,
    Default,
}

impl Field {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Driver => "driver",
            Self::Alias => "alias",
            Self::Model => "model",
            Self::BaseUrl => "base_url",
            Self::ApiKey => "api_key",
            Self::AddMore => "add_more",
            Self::Default => "default",
        }
    }
}

// ── Presets ─────────────────────────────────────────────────────────────────

// Model presets moved to crate::config_input::presets.

// Driver list and presets use the Driver enum from ozzie-core.

// CAPABILITY_LABELS removed — use ModelCapability::ALL + Display instead.

use crate::config_input::presets::{default_context_window, default_model_for, model_presets};

// env_var_for_driver removed — use Driver::env_var() instead.

// ══════════════════════════════════════════════════════════════════════════
// ProviderEntry — configures a SINGLE provider
// ══════════════════════════════════════════════════════════════════════════

/// Result of configuring a single provider.
pub struct ProviderResult {
    pub alias: String,
    pub config: ProviderConfig,
    /// Secret: (env_var, plaintext). None if no key provided or ollama.
    pub secret: Option<(String, String)>,
}

/// Configures a single LLM provider interactively.
///
/// Not a ConfigSection — used as a building block by `ProvidersSection`.
/// Also reusable for `ozzie config set provider.add`.
pub async fn configure_provider(
    section_id: &str,
    collector: &mut dyn InputCollector,
) -> anyhow::Result<Option<ProviderResult>> {
    // Phase 1: select driver
    let driver_options: Vec<SelectOption> = Driver::ALL_LLM
        .iter()
        .map(|d| SelectOption::new(d.as_str(), d.display_name()))
        .collect();
    let driver_fields = vec![FieldSpec::select(Field::Driver.as_str(), driver_options, 0)];
    let driver_values = match collector.collect(section_id, &driver_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let driver_idx = driver_values
        .get(Field::Driver.as_str())
        .and_then(FieldValue::as_index)
        .unwrap_or(0);
    let driver = Driver::ALL_LLM
        .get(driver_idx)
        .copied()
        .unwrap_or(Driver::Anthropic);

    // Phase 2: model presets + alias
    let presets = model_presets(driver);
    let mut model_options: Vec<SelectOption> = presets
        .iter()
        .map(|m| SelectOption::new(m, m))
        .collect();
    model_options.push(SelectOption::new("_custom", "Custom model"));

    let model_fields = vec![
        FieldSpec::select(Field::Model.as_str(), model_options, 0),
        FieldSpec::text_default(Field::Alias.as_str(), driver.as_str()),
    ];
    let model_values = match collector.collect(section_id, &model_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let model_idx = model_values
        .get(Field::Model.as_str())
        .and_then(FieldValue::as_index)
        .unwrap_or(0);

    let model = if model_idx < presets.len() {
        presets[model_idx].to_string()
    } else {
        let custom_fields = vec![FieldSpec::text(Field::Model.as_str()).required()];
        let custom_values = match collector.collect(section_id, &custom_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };
        custom_values
            .get(Field::Model.as_str())
            .and_then(FieldValue::as_text)
            .unwrap_or(default_model_for(driver))
            .to_string()
    };

    let alias = model_values
        .get(Field::Alias.as_str())
        .and_then(FieldValue::as_text)
        .filter(|s| !s.is_empty())
        .unwrap_or(driver.as_str())
        .to_string();

    // Phase 3: API key + base_url
    // Show API key field for all drivers except Ollama (optional for compatible drivers).
    let show_api_key = driver != Driver::Ollama;
    let mut auth_fields = Vec::new();
    if show_api_key {
        auth_fields.push(FieldSpec::secret(Field::ApiKey.as_str()));
    }
    if driver.needs_base_url() {
        let default_url = driver.default_base_url().unwrap_or("");
        auth_fields.push(FieldSpec::text_default(Field::BaseUrl.as_str(), default_url));
    }

    let mut secret = None;
    let mut base_url = None;
    let mut auth = AuthConfig::default();

    if !auth_fields.is_empty() {
        let auth_values = match collector.collect(section_id, &auth_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let env_var = driver.env_var();

        if show_api_key {
            if let Some(key) = auth_values
                .get(Field::ApiKey.as_str())
                .and_then(FieldValue::as_text)
                .filter(|s| !s.is_empty())
            {
                secret = Some((env_var.to_string(), key.to_string()));
            }
            auth = AuthConfig {
                api_key: Some(format!("${{{{ .Env.{env_var} }}}}")),
                ..Default::default()
            };
        }

        base_url = auth_values
            .get(Field::BaseUrl.as_str())
            .and_then(FieldValue::as_text)
            .filter(|s| !s.is_empty())
            .map(String::from);
    }

    // Phase 4: capabilities
    let default_caps = crate::config_input::presets::default_capabilities(driver, &model);

    let cap_options: Vec<SelectOption> = ModelCapability::ALL
        .iter()
        .map(|cap| {
            let label = cap.to_string();
            let is_default = default_caps.contains(cap);
            let display = if is_default {
                format!("{label} *")
            } else {
                label.clone()
            };
            SelectOption::new(&label, &display)
        })
        .collect();

    let default_names: Vec<String> = default_caps.iter().map(|c| c.to_string()).collect();
    let hint_key = if default_names.is_empty() {
        "capabilities".to_string()
    } else {
        format!(
            "capabilities (defaults: {}, Enter to accept)",
            default_names.join(", ")
        )
    };

    let cap_fields = vec![FieldSpec::multi_select(&hint_key, cap_options)];
    let cap_values = match collector.collect(section_id, &cap_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let capabilities: Vec<ModelCapability> = cap_values
        .get(&hint_key)
        .and_then(FieldValue::as_indices)
        .filter(|indices| !indices.is_empty())
        .map(|indices| {
            indices
                .iter()
                .filter_map(|&i| ModelCapability::ALL.get(i).copied())
                .collect()
        })
        .unwrap_or_else(|| default_caps.to_vec());

    // Phase 5: context_window detection
    let context_window = resolve_context_window(driver, &model, base_url.as_deref(), collector, section_id)?;

    Ok(Some(ProviderResult {
        alias,
        config: ProviderConfig {
            driver,
            model,
            base_url,
            auth,
            capabilities,
            context_window,
            max_concurrent: 1,
            ..Default::default()
        },
        secret,
    }))
}

/// Resolves the context window for a provider:
/// 1. Cloud providers → catalog lookup
/// 2. Ollama → probe /api/show
/// 3. OpenAI-compatible → probe /slots then /v1/models
/// 4. Fallback → ask user (default 16384)
fn resolve_context_window(
    driver: Driver,
    model: &str,
    base_url: Option<&str>,
    collector: &mut dyn InputCollector,
    section_id: &str,
) -> anyhow::Result<Option<usize>> {
    // 1. Try catalog lookup (works for all known models)
    if let Some(cw) = default_context_window(driver, model) {
        collector.show_info(&format!("Context window: {cw} tokens (from catalog)"));
        return Ok(Some(cw));
    }

    // 2. For local providers, try probing the server
    if let Some(url) = base_url
        && let Some(cw) = probe_context_window(driver, url, model)
    {
        collector.show_info(&format!("Context window: {cw} tokens (detected from server)"));
        return Ok(Some(cw));
    }

    // 3. Ask user with a sensible default
    let default = "16384";
    let fields = vec![FieldSpec::text_default("context_window", default)];
    let values = match collector.collect(section_id, &fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(Some(16384)),
    };

    let cw = values
        .get("context_window")
        .and_then(FieldValue::as_text)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(16384);

    Ok(Some(cw))
}

/// Probes a local server for its effective context window.
fn probe_context_window(driver: Driver, base_url: &str, model: &str) -> Option<usize> {
    let rt = tokio::runtime::Handle::try_current().ok()?;
    let base_url = base_url.to_string();
    let model = model.to_string();

    rt.block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok()?;

        match driver {
            Driver::Ollama => probe_ollama(&client, &base_url, &model).await,
            Driver::OpenAiCompatible | Driver::LmStudio | Driver::Vllm => {
                probe_openai_compatible(&client, &base_url).await
            }
            _ => None,
        }
    })
}

/// Ollama: POST /api/show → model_info.num_ctx or details.parameter_size fallback
async fn probe_ollama(client: &reqwest::Client, base_url: &str, model: &str) -> Option<usize> {
    let resp = client
        .post(format!("{base_url}/api/show"))
        .json(&serde_json::json!({"name": model}))
        .send()
        .await
        .ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    // Try model_info.num_ctx first (actual configured context)
    body.pointer("/model_info/num_ctx")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

/// OpenAI-compatible (llama.cpp, vLLM, LM Studio):
/// 1. GET /slots → first slot's n_ctx
/// 2. GET /v1/models → data[0].meta.n_ctx_train
async fn probe_openai_compatible(client: &reqwest::Client, base_url: &str) -> Option<usize> {
    // Try /slots first (llama.cpp)
    if let Ok(resp) = client.get(format!("{base_url}/slots")).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<serde_json::Value>().await
        && let Some(n_ctx) = body
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|slot| slot.get("n_ctx"))
            .and_then(|v| v.as_u64())
    {
        return Some(n_ctx as usize);
    }

    // Try /v1/models (some servers expose n_ctx_train in meta)
    if let Ok(resp) = client.get(format!("{base_url}/v1/models")).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<serde_json::Value>().await
        && let Some(n_ctx) = body
            .pointer("/data/0/meta/n_ctx_train")
            .and_then(|v| v.as_u64())
    {
        return Some(n_ctx as usize);
    }

    None
}

// ══════════════════════════════════════════════════════════════════════════
// ProvidersSection — list of providers + default + assemble ModelsConfig
// ══════════════════════════════════════════════════════════════════════════

pub struct ProvidersSection;

impl ProvidersSection {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ConfigSection for ProvidersSection {
    type Output = ModelsConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Providers.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        // Introspection only — build() drives the real flow
        vec![]
    }

    fn validate(&self, fragment: &Self::Output) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if fragment.providers.is_empty() {
            errors.push("at least one provider must be configured".to_string());
        }
        if !fragment.default.is_empty()
            && !fragment.providers.contains_key(&fragment.default)
        {
            errors.push(format!(
                "default provider '{}' not found in: [{}]",
                fragment.default,
                fragment
                    .providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        _current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let mut providers = HashMap::new();
        let mut secrets = Vec::new();

        // Loop: configure providers one by one
        loop {
            match configure_provider(self.id(), collector).await? {
                Some(result) => {
                    if let Some(secret) = result.secret {
                        secrets.push(secret);
                    }
                    providers.insert(result.alias, result.config);
                }
                None => {
                    if providers.is_empty() {
                        return Ok(None);
                    }
                    break;
                }
            }

            if !confirm_add_more(self.id(), collector, Field::AddMore.as_str())? {
                break;
            }
        }

        // Default provider selection (skip if only one)
        let default = if providers.len() == 1 {
            providers.keys().next().cloned().unwrap_or_default()
        } else {
            let aliases: Vec<String> = providers.keys().cloned().collect();
            let options: Vec<SelectOption> = aliases
                .iter()
                .map(|a| SelectOption::new(a, a))
                .collect();
            let default_fields =
                vec![FieldSpec::select(Field::Default.as_str(), options, 0)];
            match collector.collect(self.id(), &default_fields)? {
                CollectResult::Values(v) => {
                    let idx = v
                        .get(Field::Default.as_str())
                        .and_then(FieldValue::as_index)
                        .unwrap_or(0);
                    aliases.get(idx).cloned().unwrap_or_default()
                }
                _ => aliases.first().cloned().unwrap_or_default(),
            }
        };

        Ok(Some(BuildOutput::with_secrets(
            ModelsConfig { default, providers },
            secrets,
        )))
    }

    fn apply_field(
        &self,
        current: &Self::Output,
        field_path: &str,
        value: &str,
    ) -> anyhow::Result<BuildOutput<Self::Output>> {
        let mut cfg = current.clone();
        match field_path {
            "default" => {
                if !cfg.providers.contains_key(value) {
                    let valid: Vec<&str> = cfg.providers.keys().map(|s| s.as_str()).collect();
                    anyhow::bail!(
                        "provider '{value}' not found, available: {}",
                        valid.join(", ")
                    );
                }
                cfg.default = value.to_string();
            }
            other => anyhow::bail!("unknown field '{other}' for providers section"),
        }
        Ok(BuildOutput::new(cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::section::FieldValues;

    struct MockCollector(Vec<FieldValues>);

    impl InputCollector for MockCollector {
        fn collect(
            &mut self,
            _id: &str,
            _fields: &[FieldSpec],
        ) -> anyhow::Result<CollectResult> {
            if self.0.is_empty() {
                return Ok(CollectResult::Back);
            }
            Ok(CollectResult::Values(self.0.remove(0)))
        }
    }

    #[tokio::test]
    async fn build_single_provider() {
        let section = ProvidersSection::new();
        let mut collector = MockCollector(vec![
            // driver: anthropic (idx 0)
            HashMap::from([(Field::Driver.as_str().to_string(), FieldValue::Index(0))]),
            // model: first preset + alias
            HashMap::from([
                (Field::Model.as_str().to_string(), FieldValue::Index(0)),
                (Field::Alias.as_str().to_string(), FieldValue::Text("claude".to_string())),
            ]),
            // api key
            HashMap::from([(
                Field::ApiKey.as_str().to_string(),
                FieldValue::Text("sk-test".to_string()),
            )]),
            // capabilities: empty = use defaults
            HashMap::new(),
            // add more: no
            HashMap::from([(Field::AddMore.as_str().to_string(), FieldValue::Bool(false))]),
        ]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.default, "claude");
        assert!(output.config.providers.contains_key("claude"));
        let p = &output.config.providers["claude"];
        assert_eq!(p.driver, Driver::Anthropic);
        assert_eq!(p.model, "claude-sonnet-4-20250514");
        assert!(!p.capabilities.is_empty()); // defaults applied

        assert_eq!(output.secrets.len(), 1);
        assert_eq!(output.secrets[0].0, "ANTHROPIC_API_KEY");
    }

    #[tokio::test]
    async fn build_ollama_no_key() {
        let section = ProvidersSection::new();
        let mut collector = MockCollector(vec![
            // driver: ollama (idx 6 — after anthropic, openai, gemini, mistral, groq, xai)
            HashMap::from([(Field::Driver.as_str().to_string(), FieldValue::Index(6))]),
            // model + alias
            HashMap::from([
                (Field::Model.as_str().to_string(), FieldValue::Index(0)),
                (Field::Alias.as_str().to_string(), FieldValue::Text("local".to_string())),
            ]),
            // base_url (ollama needs it)
            HashMap::from([(
                Field::BaseUrl.as_str().to_string(),
                FieldValue::Text("http://localhost:11434".to_string()),
            )]),
            // capabilities
            HashMap::new(),
            // no more
            HashMap::from([(Field::AddMore.as_str().to_string(), FieldValue::Bool(false))]),
        ]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        let p = &output.config.providers["local"];
        assert_eq!(p.driver, Driver::Ollama);
        assert!(p.auth.api_key.is_none()); // no key for ollama

        assert!(output.secrets.is_empty());
    }

    #[test]
    fn validate_empty() {
        let section = ProvidersSection::new();
        assert!(section.validate(&ModelsConfig::default()).is_err());
    }
}
