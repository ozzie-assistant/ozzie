mod types;
mod ws;

pub use types::*;
pub use ws::WsGatewayClient;

use std::fmt;

/// Protocol-level interface to the Ozzie gateway.
///
/// Implemented by [`WsGatewayClient`] (WebSocket JSON-RPC 2.0).
/// Connectors (Discord, Mattermost, Matrix, ...) depend on this trait,
/// not on gateway internals.
#[async_trait::async_trait]
pub trait GatewayClient: Send {
    /// Open or resume a session.
    async fn open_session(&mut self, opts: OpenConversationOpts) -> Result<SessionInfo>;

    /// Forward a message from a connector platform to the gateway.
    async fn send_connector_message(&mut self, params: ConnectorMessageParams) -> Result<()>;

    /// Respond to a pending prompt (e.g. tool approval).
    async fn respond_to_prompt(&mut self, params: PromptResponseParams) -> Result<()>;

    /// Auto-approve all tool calls for the current session.
    async fn accept_all_tools(&mut self) -> Result<()>;

    /// Read the next gateway notification. Blocks until one is available.
    async fn read_notification(&mut self) -> Result<Notification>;
}

pub type Result<T> = std::result::Result<T, GatewayError>;

#[derive(Debug)]
pub enum GatewayError {
    /// WebSocket or network error.
    Connection(String),
    /// Malformed frame or unexpected response.
    Protocol(String),
    /// JSON-RPC error returned by the gateway.
    Rpc { code: i32, message: String },
    /// Connection closed.
    Closed,
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(msg) => write!(f, "connection error: {msg}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
            Self::Rpc { code, message } => write!(f, "gateway error ({code}): {message}"),
            Self::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for GatewayError {}
