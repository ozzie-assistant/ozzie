use ozzie_core::config::LayeredContextConfig;

use super::super::section::{
    BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector,
};

const ENABLE: &str = "enable";
const CUSTOMIZE: &str = "customize";
const MAX_RECENT: &str = "max_recent";
const MAX_ARCHIVES: &str = "max_archives";

pub struct MemorySection;

#[async_trait::async_trait]
impl ConfigSection for MemorySection {
    type Output = LayeredContextConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Memory.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        vec![
            FieldSpec::confirm(ENABLE, true),
            FieldSpec::confirm(CUSTOMIZE, false),
            FieldSpec::text_default(MAX_RECENT, "24"),
            FieldSpec::text_default(MAX_ARCHIVES, "12"),
        ]
    }

    fn validate(&self, _fragment: &Self::Output) -> Result<(), Vec<String>> {
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let is_currently_enabled = current.map(|c| c.is_enabled()).unwrap_or(false);

        // Phase 1: enable + customize
        let phase1 = vec![
            FieldSpec::confirm(ENABLE, current.is_none() || is_currently_enabled),
            FieldSpec::confirm(CUSTOMIZE, false),
        ];
        let values = match collector.collect(self.id(), &phase1)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let enabled = values
            .get(ENABLE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(true);

        if !enabled {
            return Ok(Some(BuildOutput::new(LayeredContextConfig {
                enabled: Some(false),
                ..Default::default()
            })));
        }

        let customize = values
            .get(CUSTOMIZE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(false);

        let (max_recent, max_archives) = if customize {
            let cur_recent = current.map(|c| c.max_recent_messages).unwrap_or(24);
            let cur_archives = current.map(|c| c.max_archives).unwrap_or(12);
            // Phase 2: custom parameters
            let phase2 = vec![
                FieldSpec::text_default(MAX_RECENT, &cur_recent.to_string()),
                FieldSpec::text_default(MAX_ARCHIVES, &cur_archives.to_string()),
            ];
            let params = match collector.collect(self.id(), &phase2)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };

            let recent = params
                .get(MAX_RECENT)
                .and_then(FieldValue::as_text)
                .and_then(|s| s.parse().ok())
                .unwrap_or(24);
            let archives = params
                .get(MAX_ARCHIVES)
                .and_then(FieldValue::as_text)
                .and_then(|s| s.parse().ok())
                .unwrap_or(12);
            (recent, archives)
        } else {
            (24, 12)
        };

        Ok(Some(BuildOutput::new(LayeredContextConfig {
            enabled: Some(true),
            max_recent_messages: max_recent,
            max_archives,
            ..Default::default()
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
            "enabled" => {
                cfg.enabled = Some(value.parse().map_err(|_| {
                    anyhow::anyhow!("invalid bool: {value}")
                })?);
            }
            "max_recent" => {
                cfg.max_recent_messages = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid number: {value}"))?;
            }
            "max_archives" => {
                cfg.max_archives = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid number: {value}"))?;
            }
            other => anyhow::bail!("unknown field '{other}' for memory section"),
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
        let section = MemorySection;
        let values = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(false)),
            (CUSTOMIZE.to_string(), FieldValue::Bool(false)),
        ]);
        let mut collector = MockCollector(vec![values]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.enabled, Some(false));
    }

    #[tokio::test]
    async fn build_defaults() {
        let section = MemorySection;
        let values = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(true)),
            (CUSTOMIZE.to_string(), FieldValue::Bool(false)),
        ]);
        let mut collector = MockCollector(vec![values]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.enabled, Some(true));
        assert_eq!(output.config.max_recent_messages, 24);
        assert_eq!(output.config.max_archives, 12);
    }

    #[tokio::test]
    async fn build_custom() {
        let section = MemorySection;
        // Phase 1: enable + customize
        let phase1 = FieldValues::from([
            (ENABLE.to_string(), FieldValue::Bool(true)),
            (CUSTOMIZE.to_string(), FieldValue::Bool(true)),
        ]);
        // Phase 2: custom params
        let phase2 = FieldValues::from([
            (MAX_RECENT.to_string(), FieldValue::Text("30".to_string())),
            (MAX_ARCHIVES.to_string(), FieldValue::Text("20".to_string())),
        ]);
        let mut collector = MockCollector(vec![phase1, phase2]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.max_recent_messages, 30);
        assert_eq!(output.config.max_archives, 20);
    }
}
