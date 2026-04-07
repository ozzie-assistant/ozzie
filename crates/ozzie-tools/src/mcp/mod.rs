//! MCP client module — connects to external MCP servers and registers their tools.
//!
//! Uses the `rmcp` crate (official Rust MCP SDK) for protocol handling.
//! - Stdio transport (spawn subprocess via `TokioChildProcess`)
//! - Tool filtering: allowed_tools / denied_tools / trusted_tools
//! - Dangerous by default (unless `dangerous: false` or tool in `trusted_tools`)

mod client;
mod setup;
mod tool;

pub use client::{McpClient, McpError};
pub use setup::{setup_mcp_servers, McpSetupResult, McpShutdownHandle};
pub use tool::McpTool;
