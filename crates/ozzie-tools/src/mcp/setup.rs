use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use ozzie_core::config::McpConfig;
use tracing::{debug, error, info};

use super::client::{McpClient, McpError};
use super::tool::McpTool;
use crate::registry::{ToolRegistry, ToolSpec};

/// Result of setting up a single MCP server.
#[derive(Debug)]
pub struct McpSetupResult {
    pub server_name: String,
    pub registered_tools: Vec<String>,
    pub error: Option<String>,
}

/// Handle for gracefully shutting down all MCP server connections.
pub struct McpShutdownHandle {
    clients: Vec<Arc<McpClient>>,
}

impl McpShutdownHandle {
    /// Gracefully shuts down all MCP server connections in parallel.
    pub async fn shutdown_all(&self) {
        let mut handles = Vec::new();
        for client in &self.clients {
            let c = client.clone();
            handles.push(tokio::spawn(async move { c.shutdown().await }));
        }
        for h in handles {
            let _ = h.await;
        }
    }
}

/// Connects to all configured MCP servers and registers their tools.
///
/// Returns setup results and a shutdown handle that must be called on graceful
/// exit to cleanly close MCP server processes (avoid EPIPE).
pub async fn setup_mcp_servers(
    config: &McpConfig,
    registry: &ToolRegistry,
) -> (Vec<McpSetupResult>, McpShutdownHandle) {
    let mut results = Vec::new();
    let mut clients = Vec::new();

    for (name, server_cfg) in &config.servers {
        match setup_one_server(name, server_cfg, registry).await {
            Ok((result, client)) => {
                info!(
                    server = %name,
                    tools = result.registered_tools.len(),
                    "MCP server connected"
                );
                clients.push(client);
                results.push(result);
            }
            Err(e) => {
                error!(server = %name, error = %e, "failed to connect MCP server");
                results.push(McpSetupResult {
                    server_name: name.clone(),
                    registered_tools: Vec::new(),
                    error: Some(e.to_string()),
                });
            }
        }
    }

    (results, McpShutdownHandle { clients })
}

async fn setup_one_server(
    name: &str,
    cfg: &ozzie_core::config::McpServerConfig,
    registry: &ToolRegistry,
) -> Result<(McpSetupResult, Arc<McpClient>), McpError> {
    let ozzie_core::config::McpServerConfig::Stdio {
        command,
        args,
        env,
        ..
    } = cfg
    else {
        return Err(McpError::Transport(
            "only `stdio` transport is currently supported".to_string(),
        ));
    };

    let command = command.as_deref().ok_or_else(|| {
        McpError::Transport(format!(
            "MCP server `{name}` has transport=stdio but no command specified"
        ))
    })?;

    let timeout = Duration::from_millis(cfg.timeout());

    debug!(server = %name, command = %command, args = ?args, "connecting to MCP server");

    let client = Arc::new(McpClient::connect_stdio(command, args, env, timeout).await?);

    let tools = client.list_tools().await?;
    debug!(server = %name, discovered = tools.len(), "MCP tools discovered");

    let allowed_set: HashSet<&str> = cfg.allowed_tools().iter().map(|s| s.as_str()).collect();
    let denied_set: HashSet<&str> = cfg.denied_tools().iter().map(|s| s.as_str()).collect();
    let trusted_set: HashSet<&str> = cfg.trusted_tools().iter().map(|s| s.as_str()).collect();
    let is_dangerous = cfg.is_dangerous();

    let mut registered_tools = Vec::new();

    for tool_def in &tools {
        let tool_name: &str = &tool_def.name;

        if !is_tool_allowed(tool_name, &allowed_set, &denied_set) {
            debug!(server = %name, tool = %tool_name, "MCP tool filtered out");
            continue;
        }

        let prefixed_name = format!("{name}__{tool_name}");
        let description = tool_def
            .description
            .as_deref()
            .unwrap_or("MCP tool")
            .to_string();
        let dangerous = is_dangerous && !trusted_set.contains(tool_name);

        // Convert the rmcp input_schema (Value) to a typed RootSchema.
        let parameters: schemars::schema::RootSchema = serde_json::to_value(tool_def.input_schema.as_ref())
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let spec = ToolSpec {
            name: prefixed_name.clone(),
            description: description.clone(),
            parameters: parameters.clone(),
            dangerous,
        };

        let mcp_tool = McpTool::new(
            name.to_string(),
            tool_name.to_string(),
            description,
            parameters,
            client.clone(),
        );

        registry.register(Box::new(mcp_tool), spec);
        registered_tools.push(prefixed_name);
    }

    info!(
        server = %name,
        total = tools.len(),
        registered = registered_tools.len(),
        "MCP tools registered"
    );

    Ok((
        McpSetupResult {
            server_name: name.to_string(),
            registered_tools,
            error: None,
        },
        client,
    ))
}

fn is_tool_allowed(
    tool_name: &str,
    allowed_set: &HashSet<&str>,
    denied_set: &HashSet<&str>,
) -> bool {
    if denied_set.contains(tool_name) {
        return false;
    }
    if !allowed_set.is_empty() {
        return allowed_set.contains(tool_name);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_allowed_no_filters() {
        let allowed = HashSet::new();
        let denied = HashSet::new();
        assert!(is_tool_allowed("any_tool", &allowed, &denied));
    }

    #[test]
    fn tool_denied_takes_priority() {
        let allowed: HashSet<&str> = ["tool_a", "tool_b"].into();
        let denied: HashSet<&str> = ["tool_a"].into();
        assert!(!is_tool_allowed("tool_a", &allowed, &denied));
        assert!(is_tool_allowed("tool_b", &allowed, &denied));
    }

    #[test]
    fn tool_allowed_whitelist() {
        let allowed: HashSet<&str> = ["tool_a"].into();
        let denied = HashSet::new();
        assert!(is_tool_allowed("tool_a", &allowed, &denied));
        assert!(!is_tool_allowed("tool_b", &allowed, &denied));
    }

    #[test]
    fn tool_denied_only() {
        let allowed = HashSet::new();
        let denied: HashSet<&str> = ["bad_tool"].into();
        assert!(!is_tool_allowed("bad_tool", &allowed, &denied));
        assert!(is_tool_allowed("good_tool", &allowed, &denied));
    }

    #[test]
    fn setup_result_debug() {
        let result = McpSetupResult {
            server_name: "test".to_string(),
            registered_tools: vec!["test__foo".to_string()],
            error: None,
        };
        assert!(format!("{result:?}").contains("test"));
    }
}
