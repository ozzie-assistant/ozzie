use ozzie_core::config::SkillsConfig;

use super::super::section::{
    BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector,
};

const ENABLE: &str = "enable";
const DIR: &str = "dir";
const ADD_MORE: &str = "add_more";

pub struct SkillsSection;

#[async_trait::async_trait]
impl ConfigSection for SkillsSection {
    type Output = SkillsConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Skills.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        vec![
            FieldSpec::confirm(ENABLE, false),
            FieldSpec::text(DIR),
        ]
    }

    fn validate(&self, _fragment: &Self::Output) -> Result<(), Vec<String>> {
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        _current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        // Phase 1: ask if enabled
        let enable_fields = vec![FieldSpec::confirm(ENABLE, false)];
        let values = match collector.collect(self.id(), &enable_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let enabled = values
            .get(ENABLE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(false);

        if !enabled {
            return Ok(Some(BuildOutput::new(SkillsConfig::default())));
        }

        // Phase 2: collect directories in a loop
        let mut dirs = Vec::new();
        loop {
            let dir_fields = vec![
                FieldSpec::text(DIR),
                FieldSpec::confirm(ADD_MORE, false),
            ];
            let dir_values = match collector.collect(self.id(), &dir_fields)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };

            if let Some(dir) = dir_values
                .get(DIR)
                .and_then(FieldValue::as_text)
                .filter(|s| !s.is_empty())
            {
                dirs.push(dir.to_string());
            }

            let add_more = dir_values
                .get(ADD_MORE)
                .and_then(FieldValue::as_bool)
                .unwrap_or(false);
            if !add_more {
                break;
            }
        }

        Ok(Some(BuildOutput::new(SkillsConfig {
            dirs,
            enabled: Vec::new(),
        })))
    }

    fn apply_field(
        &self,
        current: &Self::Output,
        field_path: &str,
        value: &str,
    ) -> anyhow::Result<BuildOutput<Self::Output>> {
        let mut cfg = current.clone();
        match field_path {
            "add_dir" => {
                if !cfg.dirs.contains(&value.to_string()) {
                    cfg.dirs.push(value.to_string());
                }
            }
            "remove_dir" => {
                cfg.dirs.retain(|d| d != value);
            }
            other => anyhow::bail!("unknown field '{other}' for skills section"),
        }
        Ok(BuildOutput::new(cfg))
    }
}

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
        let section = SkillsSection;
        let values = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(false)),
        ]);
        let mut collector = MockCollector(vec![values]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert!(output.config.dirs.is_empty());
    }

    #[tokio::test]
    async fn build_with_dirs() {
        let section = SkillsSection;
        // Phase 1: enable
        let enable = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(true)),
        ]);
        // Phase 2: one dir, no more
        let dir = FieldValues::from([
            (DIR.to_string(), FieldValue::Text("/tmp/skills".to_string())),
            (ADD_MORE.to_string(), FieldValue::Bool(false)),
        ]);
        let mut collector = MockCollector(vec![enable, dir]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.dirs, vec!["/tmp/skills"]);
    }
}
