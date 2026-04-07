use super::super::section::{
    BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector, SelectOption,
};

const LANGUAGE: &str = "language";

const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("fr", "Français")];

/// Language preference — stored in `agent.preferred_language`.
///
/// Reusable outside the wizard (e.g. `ozzie config set language fr`).
#[derive(Debug, Clone, Default)]
pub struct LanguageConfig {
    pub language: String,
}

pub struct LanguageSection {
    /// Default language (from existing config or system detection).
    default: String,
}

impl LanguageSection {
    pub fn new(default: &str) -> Self {
        Self {
            default: default.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ConfigSection for LanguageSection {
    type Output = LanguageConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Language.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        let options: Vec<SelectOption> = LANGUAGES
            .iter()
            .map(|(v, l)| SelectOption::new(v, l))
            .collect();
        let default_idx = LANGUAGES
            .iter()
            .position(|(v, _)| *v == self.default)
            .unwrap_or(0);
        vec![FieldSpec::select(LANGUAGE, options, default_idx)]
    }

    fn validate(&self, fragment: &Self::Output) -> Result<(), Vec<String>> {
        if fragment.language.is_empty() {
            return Err(vec!["language must be selected".to_string()]);
        }
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        _current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let default_idx = LANGUAGES
            .iter()
            .position(|(v, _)| *v == self.default)
            .unwrap_or(0);

        let options: Vec<SelectOption> = LANGUAGES
            .iter()
            .map(|(v, l)| SelectOption::new(v, l))
            .collect();

        let fields = vec![FieldSpec::select(LANGUAGE, options, default_idx)];
        let values = match collector.collect(self.id(), &fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let idx = values
            .get(LANGUAGE)
            .and_then(FieldValue::as_index)
            .unwrap_or(default_idx);

        let language = LANGUAGES
            .get(idx)
            .map(|(v, _)| v.to_string())
            .unwrap_or_else(|| "en".to_string());

        Ok(Some(BuildOutput::new(LanguageConfig { language })))
    }

    fn apply_field(
        &self,
        _current: &Self::Output,
        field_path: &str,
        value: &str,
    ) -> anyhow::Result<BuildOutput<Self::Output>> {
        match field_path {
            "language" => {
                if !LANGUAGES.iter().any(|(v, _)| *v == value) {
                    let valid: Vec<&str> = LANGUAGES.iter().map(|(v, _)| *v).collect();
                    anyhow::bail!("unknown language '{value}', valid: {}", valid.join(", "));
                }
                Ok(BuildOutput::new(LanguageConfig {
                    language: value.to_string(),
                }))
            }
            other => anyhow::bail!("unknown field '{other}' for language section"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use super::super::super::section::FieldValues;

    struct MockCollector(Vec<FieldValues>);
    impl InputCollector for MockCollector {
        fn collect(&mut self, _id: &str, _fields: &[FieldSpec]) -> anyhow::Result<CollectResult> {
            if self.0.is_empty() { return Ok(CollectResult::Back); }
            Ok(CollectResult::Values(self.0.remove(0)))
        }
    }

    #[tokio::test]
    async fn build_french() {
        let section = LanguageSection::new("en");
        let mut collector = MockCollector(vec![
            HashMap::from([(LANGUAGE.to_string(), FieldValue::Index(1))]),
        ]);
        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.language, "fr");
    }

    #[tokio::test]
    async fn build_default_english() {
        let section = LanguageSection::new("en");
        let mut collector = MockCollector(vec![
            HashMap::from([(LANGUAGE.to_string(), FieldValue::Index(0))]),
        ]);
        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.language, "en");
    }

    #[tokio::test]
    async fn default_from_existing() {
        let section = LanguageSection::new("fr");
        let fields = section.fields(None);
        if let super::super::super::section::FieldKind::Select { default, .. } = &fields[0].kind {
            assert_eq!(*default, 1); // fr is index 1
        }
    }
}
