//! MCP client — thin wrapper around `rmcp` for Ozzie integration.

use std::collections::HashMap;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, Tool as McpToolDef};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::warn;

/// Errors from MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("not connected")]
    NotConnected,
}

/// A single MCP client session connected to an external server via stdio.
pub struct McpClient {
    service: Mutex<Option<RunningService<RoleClient, ()>>>,
    timeout: Duration,
}

impl McpClient {
    /// Spawns an MCP server process and establishes the session.
    pub async fn connect_stdio(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Self, McpError> {
        let cmd_str = command.to_string();
        let args_clone = args.to_vec();
        let env_clone = env.clone();

        let child = TokioChildProcess::new(Command::new(&cmd_str).configure(move |cmd| {
            cmd.args(&args_clone);
            if !env_clone.is_empty() {
                cmd.envs(&env_clone);
            }
        }))
        .map_err(|e| McpError::Transport(format!("failed to spawn MCP server `{cmd_str}`: {e}")))?;

        let service = ().serve(child).await.map_err(|e| {
            McpError::Protocol(format!("MCP handshake failed: {e}"))
        })?;

        Ok(Self {
            service: Mutex::new(Some(service)),
            timeout,
        })
    }

    /// Lists all tools exposed by the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, McpError> {
        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or(McpError::NotConnected)?;
        let tools = service
            .peer()
            .list_all_tools()
            .await
            .map_err(|e| McpError::Protocol(format!("list_tools: {e}")))?;
        Ok(tools)
    }

    /// Calls a tool on the remote MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpCallResult, McpError> {
        let params = CallToolRequestParams {
            name: name.to_string().into(),
            arguments: arguments.as_object().cloned(),
            meta: None,
            task: None,
        };

        let guard = self.service.lock().await;
        let service = guard.as_ref().ok_or(McpError::NotConnected)?;

        let result = tokio::time::timeout(
            self.timeout,
            service.peer().call_tool(params),
        )
        .await
        .map_err(|_| McpError::Transport(format!("call_tool '{name}' timed out")))?
        .map_err(|e| McpError::Protocol(format!("call_tool '{name}': {e}")))?;

        let text = result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(McpCallResult {
            text,
            is_error: result.is_error.unwrap_or(false),
        })
    }

    /// Gracefully shuts down the MCP server connection.
    ///
    /// Sends a protocol-level close and waits up to 3 seconds for cleanup.
    /// Safe to call multiple times — subsequent calls are no-ops.
    pub async fn shutdown(&self) {
        let mut guard = self.service.lock().await;
        if let Some(mut service) = guard.take() {
            match service.close_with_timeout(Duration::from_secs(3)).await {
                Ok(Some(_)) => {}
                Ok(None) => warn!("MCP server did not shut down within timeout"),
                Err(e) => warn!(error = %e, "MCP server shutdown error"),
            }
        }
    }
}

/// Simplified call result for Ozzie integration.
pub struct McpCallResult {
    pub text: String,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_missing_command_fails() {
        let result = McpClient::connect_stdio(
            "/nonexistent/mcp-server-binary-zzz",
            &[],
            &HashMap::new(),
            Duration::from_secs(5),
        )
        .await;
        let err = result.err().expect("should fail");
        assert!(err.to_string().contains("transport") || err.to_string().contains("spawn"),
            "unexpected error: {err}");
    }
}
