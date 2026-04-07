use std::collections::HashMap;
use std::time::Duration;

use ozzie_core::config::{McpConfig, McpServerConfig};

use super::super::section::{
    confirm_add_more, BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue,
    InputCollector, SelectOption,
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

const ENABLE: &str = "enable";
const ADD_MORE: &str = "add_more";

enum Field {
    Name,
    Transport,
    Command,
    Url,
    TrustedTools,
}

impl Field {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Transport => "transport",
            Self::Command => "command",
            Self::Url => "url",
            Self::TrustedTools => "trusted_tools",
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// configure_mcp_server — configures a SINGLE MCP server
// ══════════════════════════════════════════════════════════════════════════

pub struct McpServerResult {
    pub name: String,
    pub config: McpServerConfig,
}

/// Configures a single MCP server interactively.
/// Reusable for `ozzie config set mcp.add`.
pub async fn configure_mcp_server(
    section_id: &str,
    collector: &mut dyn InputCollector,
) -> anyhow::Result<Option<McpServerResult>> {
    // Phase 1: name + transport
    let transport_options = vec![
        SelectOption::new("stdio", "stdio"),
        SelectOption::new("sse", "sse"),
        SelectOption::new("http", "http"),
    ];
    let basic_fields = vec![
        FieldSpec::text(Field::Name.as_str()).required(),
        FieldSpec::select(Field::Transport.as_str(), transport_options, 0),
    ];
    let basic_values = match collector.collect(section_id, &basic_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let name = basic_values
        .get(Field::Name.as_str())
        .and_then(FieldValue::as_text)
        .unwrap_or("server")
        .to_string();

    let transport_idx = basic_values
        .get(Field::Transport.as_str())
        .and_then(FieldValue::as_index)
        .unwrap_or(0);
    let transport = ["stdio", "sse", "http"][transport_idx].to_string();

    // Phase 2: transport-specific
    let detail_fields = if transport == "stdio" {
        vec![FieldSpec::text(Field::Command.as_str()).required()]
    } else {
        vec![FieldSpec::text(Field::Url.as_str()).required()]
    };
    let detail_values = match collector.collect(section_id, &detail_fields)? {
        CollectResult::Values(v) => v,
        _ => return Ok(None),
    };

    let (command, args) = if transport == "stdio" {
        let cmd_str = detail_values
            .get(Field::Command.as_str())
            .and_then(FieldValue::as_text)
            .unwrap_or("");
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            (Some(cmd_str.to_string()), Vec::new())
        } else {
            (
                Some(parts[0].to_string()),
                parts[1..].iter().map(|s| s.to_string()).collect(),
            )
        }
    } else {
        (None, Vec::new())
    };

    let url = if transport != "stdio" {
        detail_values
            .get(Field::Url.as_str())
            .and_then(FieldValue::as_text)
            .filter(|s| !s.is_empty())
            .map(String::from)
    } else {
        None
    };

    // Phase 3: probe (stdio only)
    let mut trusted_tools = Vec::new();
    if transport == "stdio"
        && let Some(ref cmd) = command
    {
        match probe_stdio(cmd, &args).await {
            Ok(tools) if !tools.is_empty() => {
                let options: Vec<SelectOption> =
                    tools.iter().map(|t| SelectOption::new(t, t)).collect();
                let trusted_fields =
                    vec![FieldSpec::multi_select(Field::TrustedTools.as_str(), options)];
                if let Ok(CollectResult::Values(tv)) =
                    collector.collect(section_id, &trusted_fields)
                    && let Some(indices) =
                        tv.get(Field::TrustedTools.as_str()).and_then(FieldValue::as_indices)
                {
                    trusted_tools = indices
                        .iter()
                        .filter_map(|&i| tools.get(i).cloned())
                        .collect();
                }
            }
            Ok(_) => tracing::debug!(server = %name, "probe returned no tools"),
            Err(e) => tracing::warn!(server = %name, error = %e, "probe failed"),
        }
    }

    let config = match transport.as_str() {
        "stdio" => McpServerConfig::Stdio {
            command,
            args,
            env: HashMap::new(),
            dangerous: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            trusted_tools,
            timeout: 30000,
        },
        "sse" => McpServerConfig::Sse {
            url,
            dangerous: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            trusted_tools,
            timeout: 30000,
        },
        _ => McpServerConfig::Http {
            url,
            dangerous: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            trusted_tools,
            timeout: 30000,
        },
    };

    Ok(Some(McpServerResult { name, config }))
}

async fn probe_stdio(command: &str, args: &[String]) -> anyhow::Result<Vec<String>> {
    let env = HashMap::new();
    let client =
        ozzie_tools::mcp::McpClient::connect_stdio(command, args, &env, PROBE_TIMEOUT).await?;
    let tools = client.list_tools().await?;
    let mut names: Vec<String> = tools.into_iter().map(|t| t.name.to_string()).collect();
    names.sort();
    Ok(names)
}

// ══════════════════════════════════════════════════════════════════════════
// McpServersSection — list of MCP servers + assemble McpConfig
// ══════════════════════════════════════════════════════════════════════════

pub struct McpServersSection;

#[async_trait::async_trait]
impl ConfigSection for McpServersSection {
    type Output = McpConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Mcp.as_str()
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
        // Ask if user wants MCP at all
        let enable_fields = vec![FieldSpec::confirm(ENABLE, false)];
        let enable_values = match collector.collect(self.id(), &enable_fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(Some(BuildOutput::new(McpConfig::default()))),
        };
        if !enable_values
            .get(ENABLE)
            .and_then(FieldValue::as_bool)
            .unwrap_or(false)
        {
            return Ok(Some(BuildOutput::new(McpConfig::default())));
        }

        let mut servers = HashMap::new();

        loop {
            match configure_mcp_server(self.id(), collector).await? {
                Some(result) => {
                    servers.insert(result.name, result.config);
                }
                None => break,
            }

            if !confirm_add_more(self.id(), collector, ADD_MORE)? {
                break;
            }
        }

        Ok(Some(BuildOutput::new(McpConfig { servers })))
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

    fn enable() -> FieldValues {
        HashMap::from([(ENABLE.to_string(), FieldValue::Bool(true))])
    }

    fn disable() -> FieldValues {
        HashMap::from([(ENABLE.to_string(), FieldValue::Bool(false))])
    }

    #[tokio::test]
    async fn build_disabled() {
        let section = McpServersSection;
        let mut collector = MockCollector(vec![disable()]);
        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert!(output.config.servers.is_empty());
    }

    #[tokio::test]
    async fn build_stdio_server() {
        let section = McpServersSection;
        let mut collector = MockCollector(vec![
            enable(),
            // name + transport
            HashMap::from([
                (Field::Name.as_str().to_string(), FieldValue::Text("test".to_string())),
                (Field::Transport.as_str().to_string(), FieldValue::Index(0)),
            ]),
            // command
            HashMap::from([(
                Field::Command.as_str().to_string(),
                FieldValue::Text("echo hello".to_string()),
            )]),
            // no more
            HashMap::from([(ADD_MORE.to_string(), FieldValue::Bool(false))]),
        ]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert!(output.config.servers.contains_key("test"));
        assert!(matches!(output.config.servers["test"], McpServerConfig::Stdio { .. }));
    }

    #[tokio::test]
    async fn build_http_server() {
        let section = McpServersSection;
        let mut collector = MockCollector(vec![
            enable(),
            HashMap::from([
                (Field::Name.as_str().to_string(), FieldValue::Text("api".to_string())),
                (Field::Transport.as_str().to_string(), FieldValue::Index(2)),
            ]),
            HashMap::from([(
                Field::Url.as_str().to_string(),
                FieldValue::Text("https://mcp.example.com".to_string()),
            )]),
            HashMap::from([(ADD_MORE.to_string(), FieldValue::Bool(false))]),
        ]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        match &output.config.servers["api"] {
            McpServerConfig::Http { url, .. } => {
                assert_eq!(url.as_deref(), Some("https://mcp.example.com"));
            }
            other => panic!("expected Http variant, got {other:?}"),
        }
    }
}
