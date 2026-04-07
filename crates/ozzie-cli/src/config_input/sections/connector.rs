use std::collections::HashMap;

use ozzie_core::config::{ConnectorProcessConfig, ConnectorsConfig};

use super::super::section::{
    confirm_add_more, BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector,
    SelectOption,
};

const PRESET: &str = "preset";
const NAME: &str = "name";
const COMMAND: &str = "command";
const AUTO_PAIR: &str = "auto_pair";
const RESTART: &str = "restart";
const ENABLE: &str = "enable";
const ADD_MORE: &str = "add_more";

const PRESET_TUI: usize = 0;
const PRESET_DISCORD: usize = 1;
const PRESET_FILE: usize = 2;

/// Result of configuring a single connector.
pub struct ConnectorResult {
    pub name: String,
    pub config: ConnectorProcessConfig,
}

/// Configures a single connector interactively.
pub async fn configure_connector(
    section_id: &str,
    collector: &mut dyn InputCollector,
) -> anyhow::Result<Option<ConnectorResult>> {
    let preset_options = vec![
        SelectOption::new("tui", "TUI (terminal)"),
        SelectOption::new("discord", "Discord"),
        SelectOption::new("file", "File bridge (dev)"),
        SelectOption::new("custom", "Custom"),
    ];
    let preset_fields = vec![FieldSpec::select(PRESET, preset_options, 0)];
    let preset_values = match collector.collect(section_id, &preset_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let preset_idx = preset_values
        .get(PRESET)
        .and_then(FieldValue::as_index)
        .unwrap_or(0);

    match preset_idx {
        PRESET_TUI => Ok(Some(ConnectorResult {
            name: "tui".to_string(),
            config: ConnectorProcessConfig {
                command: "ozzie".to_string(),
                args: vec!["tui".to_string()],
                auto_pair: true,
                ..Default::default()
            },
        })),
        PRESET_DISCORD => Ok(Some(ConnectorResult {
            name: "discord".to_string(),
            config: ConnectorProcessConfig {
                command: "ozzie".to_string(),
                args: vec!["connector".into(), "discord".into(), "start".into()],
                auto_pair: true,
                restart: true,
                ..Default::default()
            },
        })),
        PRESET_FILE => Ok(Some(ConnectorResult {
            name: "file".to_string(),
            config: ConnectorProcessConfig {
                command: "ozzie".to_string(),
                args: vec!["connector".into(), "file".into(), "start".into()],
                auto_pair: true,
                ..Default::default()
            },
        })),
        _ => {
            let custom_fields = vec![
                FieldSpec::text(NAME).required(),
                FieldSpec::text(COMMAND).required(),
                FieldSpec::confirm(AUTO_PAIR, true),
                FieldSpec::confirm(RESTART, false),
            ];
            let custom_values = match collector.collect(section_id, &custom_fields)? {
                CollectResult::Values(v) => v,
                _ => return Ok(None),
            };

            let name = custom_values
                .get(NAME)
                .and_then(FieldValue::as_text)
                .unwrap_or("custom")
                .to_string();
            let cmd_str = custom_values
                .get(COMMAND)
                .and_then(FieldValue::as_text)
                .unwrap_or("");
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            let (command, args) = if parts.is_empty() {
                (cmd_str.to_string(), Vec::new())
            } else {
                (
                    parts[0].to_string(),
                    parts[1..].iter().map(|s| s.to_string()).collect(),
                )
            };
            let auto_pair = custom_values
                .get(AUTO_PAIR)
                .and_then(FieldValue::as_bool)
                .unwrap_or(true);
            let restart = custom_values
                .get(RESTART)
                .and_then(FieldValue::as_bool)
                .unwrap_or(false);

            Ok(Some(ConnectorResult {
                name,
                config: ConnectorProcessConfig {
                    command,
                    args,
                    auto_pair,
                    restart,
                    ..Default::default()
                },
            }))
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════

pub struct ConnectorsSection;

#[async_trait::async_trait]
impl ConfigSection for ConnectorsSection {
    type Output = ConnectorsConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Connectors.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, _current: Option<&Self::Output>) -> Vec<FieldSpec> {
        vec![]
    }

    fn validate(&self, _fragment: &Self::Output) -> Result<(), Vec<String>> {
        Ok(())
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        _current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let enable_fields = vec![FieldSpec::confirm(ENABLE, false)];
        let enable_values = match collector.collect(self.id(), &enable_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(Some(BuildOutput::new(ConnectorsConfig::default()))),
        };
        if !enable_values
            .get(ENABLE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(false)
        {
            return Ok(Some(BuildOutput::new(ConnectorsConfig::default())));
        }

        let mut map = HashMap::new();
        loop {
            match configure_connector(self.id(), collector).await? {
                Some(r) => {
                    map.insert(r.name, r.config);
                }
                None => break,
            }
            if !confirm_add_more(self.id(), collector, ADD_MORE)? {
                break;
            }
        }

        Ok(Some(BuildOutput::new(ConnectorsConfig(map))))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::section::FieldValues;
    use super::*;

    struct MockCollector(Vec<FieldValues>);
    impl InputCollector for MockCollector {
        fn collect(&mut self, _id: &str, _fields: &[FieldSpec]) -> anyhow::Result<CollectResult> {
            if self.0.is_empty() {
                return Ok(CollectResult::Back);
            }
            Ok(CollectResult::Values(self.0.remove(0)))
        }
    }

    #[tokio::test]
    async fn build_tui_preset() {
        let section = ConnectorsSection;
        let mut c = MockCollector(vec![
            HashMap::from([(ENABLE.to_string(), FieldValue::Bool(true))]),
            HashMap::from([(PRESET.to_string(), FieldValue::Index(PRESET_TUI))]),
            HashMap::from([(ADD_MORE.to_string(), FieldValue::Bool(false))]),
        ]);
        let output = section.build(&mut c, None).await.unwrap().unwrap();
        assert!(output.config.0.contains_key("tui"));
    }

    #[tokio::test]
    async fn build_disabled() {
        let section = ConnectorsSection;
        let mut c = MockCollector(vec![HashMap::from([(
            ENABLE.to_string(),
            FieldValue::Bool(false),
        )])]);
        let output = section.build(&mut c, None).await.unwrap().unwrap();
        assert!(output.config.0.is_empty());
    }
}
