use ozzie_tools::registry::ToolRegistry;

/// MCP tool definition for external clients.
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// MCP tool call result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpToolResult {
    pub content: String,
    pub is_error: bool,
}

/// Converts the tool registry into MCP-compatible tool definitions.
pub fn list_tools(registry: &ToolRegistry) -> Vec<McpTool> {
    let names = registry.names();
    let mut tools = Vec::new();

    for name in &names {
        let Some(spec) = registry.spec(name) else {
            continue;
        };

        let input_schema = serde_json::to_value(&spec.parameters)
            .unwrap_or(serde_json::json!({"type": "object"}));
        tools.push(McpTool {
            name: spec.name.clone(),
            description: spec.description.clone(),
            input_schema,
        });
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_empty_registry() {
        let registry = ToolRegistry::new();
        let tools = list_tools(&registry);
        assert!(tools.is_empty());
    }
}
