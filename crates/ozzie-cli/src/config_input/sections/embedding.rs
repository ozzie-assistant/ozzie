use ozzie_core::config::{AuthConfig, Driver, EmbeddingConfig};

use super::super::section::{
    BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector, SelectOption,
};

const ENABLE: &str = "enable";
const DRIVER: &str = "driver";
const MODEL: &str = "model";
const DIMS: &str = "dims";
const API_KEY: &str = "api_key";
const BASE_URL: &str = "base_url";

// Embedding model presets moved to crate::config_input::presets.

/// Embedding config section.
pub struct EmbeddingSection;

impl EmbeddingSection {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ConfigSection for EmbeddingSection {
    type Output = EmbeddingConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Embedding.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        let driver_options: Vec<SelectOption> = Driver::ALL_EMBEDDING
            .iter()
            .map(|d| SelectOption::new(d.as_str(), d.display_name()))
            .collect();

        vec![
            FieldSpec::confirm(ENABLE, true),
            FieldSpec::select(DRIVER, driver_options, 0),
            FieldSpec::text(MODEL),
            FieldSpec::text(DIMS),
            FieldSpec::secret(API_KEY),
        ]
    }

    fn validate(&self, fragment: &Self::Output) -> Result<(), Vec<String>> {
        if fragment.enabled == Some(true) && fragment.driver.is_none() {
            return Err(vec!["embedding driver must be set when enabled".to_string()]);
        }
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let is_currently_enabled = current.map(|c| c.is_enabled()).unwrap_or(false);

        // Phase 1: enable
        let enable_default = current.is_none() || is_currently_enabled;
        let enable_fields = vec![FieldSpec::confirm(ENABLE, enable_default)];
        let values = match collector.collect(self.id(), &enable_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let enabled = values
            .get(ENABLE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(true);

        if !enabled {
            return Ok(Some(BuildOutput::new(EmbeddingConfig {
                enabled: Some(false),
                ..Default::default()
            })));
        }

        // Phase 2: driver selection
        let driver_options: Vec<SelectOption> = Driver::ALL_EMBEDDING
            .iter()
            .map(|d| SelectOption::new(d.as_str(), d.display_name()))
            .collect();
        let current_driver_idx = current
            .and_then(|c| c.driver)
            .and_then(|d| Driver::ALL_EMBEDDING.iter().position(|&x| x == d))
            .unwrap_or(0);
        let driver_fields = vec![FieldSpec::select(DRIVER, driver_options, current_driver_idx)];
        let driver_values = match collector.collect(self.id(), &driver_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let driver_idx = driver_values
            .get(DRIVER)
            .and_then(FieldValue::as_index)
            .unwrap_or(current_driver_idx);
        let driver = Driver::ALL_EMBEDDING
            .get(driver_idx)
            .copied()
            .unwrap_or(Driver::OpenAi);

        // Phase 3: model presets for chosen driver
        let presets = presets_for_driver(driver);
        let mut model_options: Vec<SelectOption> = presets
            .iter()
            .map(|(m, d)| SelectOption::new(m, &format!("{m} ({d}d)")))
            .collect();
        model_options.push(SelectOption::new("_custom", "Custom model"));

        let model_fields = vec![FieldSpec::select(MODEL, model_options, 0)];
        let model_values = match collector.collect(self.id(), &model_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let model_idx = model_values
            .get(MODEL)
            .and_then(FieldValue::as_index)
            .unwrap_or(0);

        let (model, dims) = if model_idx < presets.len() {
            let (m, d) = presets[model_idx];
            (m.to_string(), Some(d))
        } else {
            // Custom model: ask for name and dimensions
            let custom_fields = vec![
                FieldSpec::text(MODEL).required(),
                FieldSpec::text(DIMS),
            ];
            let custom_values = match collector.collect(self.id(), &custom_fields)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };
            let m = custom_values
                .get(MODEL)
                .and_then(FieldValue::as_text)
                .unwrap_or("")
                .to_string();
            let d = custom_values
                .get(DIMS)
                .and_then(FieldValue::as_text)
                .and_then(|s| s.parse().ok());
            (m, d)
        };

        // Phase 4: base_url (for ollama and openai-compatible)
        let base_url = if driver.needs_base_url() {
            let default_url = driver.default_base_url().unwrap_or("");
            let url_fields = vec![FieldSpec::text_default(BASE_URL, default_url)];
            let url_values = match collector.collect(self.id(), &url_fields)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };
            url_values
                .get(BASE_URL)
                .and_then(FieldValue::as_text)
                .filter(|s| !s.is_empty())
                .map(String::from)
        } else {
            None
        };

        // Phase 5: API key (skip for ollama)
        let mut secrets = Vec::new();
        let auth = if driver.needs_api_key() {
            let key_fields = vec![FieldSpec::secret(API_KEY)];
            let key_values = match collector.collect(self.id(), &key_fields)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };

            let env_var = driver.env_var();

            if let Some(key) = key_values
                .get(API_KEY)
                .and_then(FieldValue::as_text)
                .filter(|s| !s.is_empty())
            {
                secrets.push((env_var.to_string(), key.to_string()));
            }

            AuthConfig {
                api_key: Some(format!("${{{{ .Env.{env_var} }}}}")),
                ..Default::default()
            }
        } else {
            AuthConfig::default()
        };

        Ok(Some(BuildOutput::with_secrets(
            EmbeddingConfig {
                enabled: Some(true),
                driver: Some(driver),
                model,
                dims,
                base_url,
                auth,
                ..Default::default()
            },
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
            "enabled" => {
                cfg.enabled = Some(value.parse().map_err(|_| {
                    anyhow::anyhow!("invalid bool: {value}")
                })?);
            }
            "driver" => {
                cfg.driver = Some(value.parse::<Driver>().map_err(|e| anyhow::anyhow!("{e}"))?);
            }
            "model" => cfg.model = value.to_string(),
            "dims" => {
                cfg.dims = if value.is_empty() {
                    None
                } else {
                    Some(
                        value
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid number: {value}"))?,
                    )
                };
            }
            other => anyhow::bail!("unknown field '{other}' for embedding section"),
        }
        self.validate(&cfg).map_err(|e| anyhow::anyhow!("{}", e.join(", ")))?;
        Ok(BuildOutput::new(cfg))
    }
}

use crate::config_input::presets::embedding_presets as presets_for_driver;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::section::{CollectResult, FieldValues};

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
    async fn build_disabled() {
        let section = EmbeddingSection::new();
        let enable = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(false)),
        ]);
        let mut collector = MockCollector(vec![enable]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.enabled, Some(false));
        assert!(output.secrets.is_empty());
    }

    #[tokio::test]
    async fn build_openai_default() {
        let section = EmbeddingSection::new();
        // Phase 1: enable
        let enable = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(true)),
        ]);
        // Phase 2: driver = openai (index 0)
        let driver = FieldValues::from([
            (DRIVER.to_string(), FieldValue::Index(0)),
        ]);
        // Phase 3: model = first preset (index 0)
        let model = FieldValues::from([
            (MODEL.to_string(), FieldValue::Index(0)),
        ]);
        // Phase 4: API key
        let key = FieldValues::from([
            (API_KEY.to_string(), FieldValue::Text("sk-test".to_string())),
        ]);
        let mut collector = MockCollector(vec![enable, driver, model, key]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.driver, Some(Driver::OpenAi));
        assert_eq!(output.config.model, "text-embedding-3-small");
        assert_eq!(output.config.dims, Some(1536));

        assert_eq!(output.secrets.len(), 1);
        assert_eq!(output.secrets[0].0, "OPENAI_API_KEY");
        assert_eq!(output.secrets[0].1, "sk-test");
    }

    #[tokio::test]
    async fn build_ollama_no_key() {
        let section = EmbeddingSection::new();
        let enable = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(true)),
        ]);
        let driver = FieldValues::from([
            (DRIVER.to_string(), FieldValue::Index(3)), // ollama
        ]);
        let model = FieldValues::from([
            (MODEL.to_string(), FieldValue::Index(0)), // nomic-embed-text
        ]);
        let base_url = FieldValues::from([
            (BASE_URL.to_string(), FieldValue::Text("http://localhost:11434".to_string())),
        ]);
        let mut collector = MockCollector(vec![enable, driver, model, base_url]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.driver, Some(Driver::Ollama));
        assert_eq!(output.config.model, "nomic-embed-text");
        assert_eq!(output.config.base_url.as_deref(), Some("http://localhost:11434"));
        assert!(output.secrets.is_empty());
    }
}
