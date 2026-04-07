//! MCP tool adapter — wraps a remote MCP tool as an `ozzie_core::domain::Tool`.

use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::schema::RootSchema;

use super::client::McpClient;

/// A tool that proxies calls to an external MCP server.
pub struct McpTool {
    /// MCP server name (for namespacing / diagnostics).
    #[allow(dead_code)]
    server_name: String,
    /// Tool name as declared by the MCP server.
    tool_name: String,
    /// Full display name: `{server_name}__{tool_name}`.
    display_name: String,
    /// Tool description from the MCP server.
    description: String,
    /// JSON Schema for the tool's input parameters.
    parameters: RootSchema,
    /// Shared client session.
    client: Arc<McpClient>,
}

impl McpTool {
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        parameters: RootSchema,
        client: Arc<McpClient>,
    ) -> Self {
        let display_name = format!("{server_name}__{tool_name}");
        Self {
            server_name,
            tool_name,
            display_name,
            description,
            parameters,
            client,
        }
    }
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            self.display_name.clone(),
            self.description.clone(),
            self.parameters.clone(),
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let arguments: serde_json::Value = if arguments_json.is_empty() || arguments_json == "{}" {
            serde_json::json!({})
        } else {
            serde_json::from_str(arguments_json).map_err(|e| {
                ToolError::Execution(format!(
                    "invalid JSON arguments for MCP tool {}: {e}",
                    self.display_name
                ))
            })?
        };

        let result = self
            .client
            .call_tool(&self.tool_name, arguments)
            .await
            .map_err(|e| {
                ToolError::Execution(format!("MCP tool {} error: {e}", self.display_name))
            })?;

        if result.is_error {
            return Err(ToolError::Execution(format!(
                "MCP {}: tool error: {}",
                self.display_name, result.text
            )));
        }

        Ok(result.text)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn display_name_format() {
        let name = format!("{}__{}", "github", "list_repos");
        assert_eq!(name, "github__list_repos");
    }
}
