use std::sync::Arc;

use clap::Args;
use ozzie_core::domain::{Tool, ToolLookup};
use ozzie_tools::{ToolRegistry, ToolSpec};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ToolsCapability,
};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::StreamableHttpService;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, service::RequestContext};

/// Expose tools via MCP protocol.
#[derive(Args)]
pub struct McpServeArgs {
    /// Path to tool definitions JSON file.
    #[arg(long)]
    tools_file: Option<String>,

    /// Transport mode: stdio (default) or http.
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// Port for HTTP transport (default: 8808).
    #[arg(long, default_value = "8808")]
    port: u16,
}

pub async fn run(args: McpServeArgs) -> anyhow::Result<()> {
    let registry = Arc::new(ToolRegistry::new());

    if let Some(ref path) = args.tools_file {
        let content = std::fs::read_to_string(path)?;
        let specs: Vec<ToolSpec> = serde_json::from_str(&content)?;
        for spec in specs {
            registry.register(
                Box::new(StubTool {
                    name: spec.name.clone(),
                    description: spec.description.clone(),
                }),
                spec,
            );
        }
    } else {
        ozzie_tools::native::register_all(&registry, None);
    }

    let tool_count = registry.names().len();

    match args.transport.as_str() {
        "stdio" => run_stdio(registry, tool_count).await,
        "http" => run_http(registry, tool_count, args.port).await,
        other => anyhow::bail!("unsupported transport: {other} (expected: stdio, http)"),
    }
}

async fn run_stdio(registry: Arc<ToolRegistry>, tool_count: usize) -> anyhow::Result<()> {
    eprintln!("MCP server ready on stdio ({tool_count} tools)");

    let handler = OzzieMcpServer { registry };
    let server = handler
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server failed to start: {e}"))?;

    server
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

async fn run_http(
    registry: Arc<ToolRegistry>,
    tool_count: usize,
    port: u16,
) -> anyhow::Result<()> {
    eprintln!("MCP server ready on http://0.0.0.0:{port} ({tool_count} tools)");

    let handler = OzzieMcpServer { registry };
    let config = rmcp::transport::StreamableHttpServerConfig::default();
    let session_manager = Arc::new(LocalSessionManager::default());

    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        session_manager,
        config,
    );

    let app = axum::Router::new().fallback_service(service);

    let tcp = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| anyhow::anyhow!("bind port {port}: {e}"))?;

    axum::serve(tcp, app)
        .await
        .map_err(|e| anyhow::anyhow!("HTTP server error: {e}"))?;

    Ok(())
}

/// MCP server handler backed by the Ozzie ToolRegistry.
#[derive(Clone)]
struct OzzieMcpServer {
    registry: Arc<ToolRegistry>,
}

impl ServerHandler for OzzieMcpServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: None,
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "ozzie".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools: Vec<rmcp::model::Tool> = self
            .registry
            .all_specs()
            .into_iter()
            .map(spec_to_rmcp_tool)
            .collect();

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name: &str = &request.name;

        let tools = self.registry.tools_by_names(&[tool_name.to_string()]);

        let Some(tool_impl) = tools.first() else {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "tool not found: {tool_name}"
            ))]));
        };

        let args_str = match &request.arguments {
            Some(map) => serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };

        match tool_impl.run(&args_str).await {
            Ok(result) => Ok(CallToolResult::success(vec![Content::text(result)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }
}

fn spec_to_rmcp_tool(spec: ToolSpec) -> rmcp::model::Tool {
    let input_schema = serde_json::to_value(&spec.parameters)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    rmcp::model::Tool {
        name: spec.name.into(),
        description: Some(spec.description.into()),
        input_schema: Arc::new(input_schema),
        title: None,
        output_schema: None,
        annotations: None,
        execution: None,
        icons: None,
        meta: None,
    }
}

use ozzie_core::domain::{ToolError, ToolInfo};

struct StubTool {
    name: String,
    description: String,
}

#[async_trait::async_trait]
impl Tool for StubTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::new(self.name.clone(), self.description.clone())
    }

    async fn run(&self, _arguments_json: &str) -> Result<String, ToolError> {
        Err(ToolError::Execution(format!(
            "stub tool '{}' cannot execute",
            self.name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_parameters_is_json_schema() {
        // Deserialize a full ToolSpec from JSON (parameters as RootSchema).
        let spec: ToolSpec = serde_json::from_value(serde_json::json!({
            "name": "read_file",
            "description": "Read a file",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            },
            "dangerous": false
        }))
        .expect("valid ToolSpec");

        let v = serde_json::to_value(&spec.parameters).unwrap();
        assert_eq!(v["type"], "object");
        assert!(v["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("path")));
    }
}
