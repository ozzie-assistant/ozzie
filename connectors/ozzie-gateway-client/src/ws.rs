use std::collections::VecDeque;

use futures_util::{SinkExt, StreamExt};
use ozzie_types::EventKind;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::debug;

use crate::types::*;
use crate::{GatewayClient, GatewayError, Result};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Internal JSON-RPC 2.0 frame.
#[derive(Deserialize)]
struct RawFrame {
    id: Option<String>,
    method: Option<String>,
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
    params: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i32,
    message: String,
}

// JSON-RPC method names — single source of truth for client-side calls.
const METHOD_OPEN_CONVERSATION: &str = "open_conversation";
const METHOD_SEND_CONNECTOR_MESSAGE: &str = "send_connector_message";
const METHOD_PROMPT_RESPONSE: &str = "prompt_response";
const METHOD_ACCEPT_ALL_TOOLS: &str = "accept_all_tools";
const METHOD_NEW_CONVERSATION: &str = "new_conversation";
const METHOD_SWITCH_CONVERSATION: &str = "switch_conversation";
const METHOD_LIST_CONVERSATIONS: &str = "list_conversations";
const METHOD_CLOSE_CONVERSATION: &str = "close_conversation";
const MAX_NOTIFICATION_BUFFER: usize = 1024;

/// WebSocket-backed gateway client.
///
/// Speaks JSON-RPC 2.0 over a single WebSocket connection.
/// Notifications received while waiting for a response are buffered
/// and returned by subsequent [`GatewayClient::read_notification`] calls.
pub struct WsGatewayClient {
    ws: WsStream,
    next_id: u64,
    notification_buffer: VecDeque<Notification>,
}

impl WsGatewayClient {
    /// Connect to the Ozzie gateway.
    ///
    /// The `token` (if provided) is appended as a query parameter.
    pub async fn connect(url: &str, token: Option<&str>) -> Result<Self> {
        let full_url = match token {
            Some(t) if !t.is_empty() => {
                let sep = if url.contains('?') { '&' } else { '?' };
                format!("{url}{sep}token={t}")
            }
            _ => url.to_string(),
        };

        debug!(url = %full_url, "connecting to gateway");

        let (ws, _) = connect_async(&full_url)
            .await
            .map_err(|e| GatewayError::Connection(e.to_string()))?;

        Ok(Self {
            ws,
            next_id: 0,
            notification_buffer: VecDeque::new(),
        })
    }

    /// Send a JSON-RPC request and wait for the matching response.
    /// Notifications received in the meantime are buffered.
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let id = format!("req_{}", self.next_id);
        self.next_id += 1;

        let frame = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        self.ws
            .send(WsMessage::Text(frame.to_string().into()))
            .await
            .map_err(|e| GatewayError::Connection(e.to_string()))?;

        loop {
            let frame = self.read_raw_frame().await?;

            if frame.id.as_deref() == Some(id.as_str()) {
                if let Some(err) = frame.error {
                    return Err(GatewayError::Rpc {
                        code: err.code,
                        message: err.message,
                    });
                }
                return Ok(frame.result.unwrap_or(serde_json::Value::Null));
            }

            // Not our response — must be a notification, buffer it.
            if let Some(method) = frame.method
                && self.notification_buffer.len() < MAX_NOTIFICATION_BUFFER
            {
                let params = frame.params.unwrap_or(serde_json::Value::Null);
                self.notification_buffer
                    .push_back(parse_notification(&method, params));
            }
        }
    }

    /// Read and deserialize the next WebSocket text frame.
    async fn read_raw_frame(&mut self) -> Result<RawFrame> {
        loop {
            let msg = self
                .ws
                .next()
                .await
                .ok_or(GatewayError::Closed)?
                .map_err(|e| GatewayError::Connection(e.to_string()))?;

            match msg {
                WsMessage::Text(text) => {
                    return serde_json::from_str(&text)
                        .map_err(|e| GatewayError::Protocol(format!("invalid frame: {e}")));
                }
                WsMessage::Close(_) => return Err(GatewayError::Closed),
                _ => continue,
            }
        }
    }
}

#[async_trait::async_trait]
impl GatewayClient for WsGatewayClient {
    async fn open_session(&mut self, opts: OpenConversationOpts) -> Result<SessionInfo> {
        let params =
            serde_json::to_value(&opts).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        let result = self.call(METHOD_OPEN_CONVERSATION, params).await?;
        serde_json::from_value(result)
            .map_err(|e| GatewayError::Protocol(format!("invalid session response: {e}")))
    }

    async fn send_connector_message(&mut self, params: ConnectorMessageParams) -> Result<()> {
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        self.call(METHOD_SEND_CONNECTOR_MESSAGE, value).await?;
        Ok(())
    }

    async fn respond_to_prompt(&mut self, params: PromptResponseParams) -> Result<()> {
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        self.call(METHOD_PROMPT_RESPONSE, value).await?;
        Ok(())
    }

    async fn accept_all_tools(&mut self) -> Result<()> {
        self.call(METHOD_ACCEPT_ALL_TOOLS, serde_json::json!({}))
            .await?;
        Ok(())
    }

    async fn new_conversation(&mut self, title: Option<String>) -> Result<SessionInfo> {
        let params = NewConversationParams { title };
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        let result = self.call(METHOD_NEW_CONVERSATION, value).await?;
        serde_json::from_value(result)
            .map_err(|e| GatewayError::Protocol(format!("invalid new_conversation response: {e}")))
    }

    async fn switch_conversation(&mut self, conversation_id: &str) -> Result<SwitchedResult> {
        let params = SwitchConversationParams {
            conversation_id: conversation_id.to_string(),
        };
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        let result = self.call(METHOD_SWITCH_CONVERSATION, value).await?;
        serde_json::from_value(result).map_err(|e| {
            GatewayError::Protocol(format!("invalid switch_conversation response: {e}"))
        })
    }

    async fn list_conversations(
        &mut self,
        include_archived: bool,
    ) -> Result<Vec<ConversationSummaryDto>> {
        let params = ListConversationsParams { include_archived };
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        let result = self.call(METHOD_LIST_CONVERSATIONS, value).await?;
        let parsed: ConversationsListResult = serde_json::from_value(result).map_err(|e| {
            GatewayError::Protocol(format!("invalid list_conversations response: {e}"))
        })?;
        Ok(parsed.conversations)
    }

    async fn close_conversation(
        &mut self,
        conversation_id: Option<&str>,
    ) -> Result<ArchivedResult> {
        let params = CloseConversationParams {
            conversation_id: conversation_id.map(String::from),
        };
        let value =
            serde_json::to_value(&params).map_err(|e| GatewayError::Protocol(e.to_string()))?;
        let result = self.call(METHOD_CLOSE_CONVERSATION, value).await?;
        serde_json::from_value(result).map_err(|e| {
            GatewayError::Protocol(format!("invalid close_conversation response: {e}"))
        })
    }

    async fn read_notification(&mut self) -> Result<Notification> {
        if let Some(n) = self.notification_buffer.pop_front() {
            return Ok(n);
        }

        loop {
            let frame = self.read_raw_frame().await?;

            if frame.id.is_some() {
                continue;
            }

            if let Some(method) = frame.method {
                let params = frame.params.unwrap_or(serde_json::Value::Null);
                return Ok(parse_notification(&method, params));
            }
        }
    }
}

// ---- Notification parsing ----

fn parse_notification(method: &str, params: serde_json::Value) -> Notification {
    let Some(kind) = EventKind::parse(method) else {
        return Notification::Unknown {
            method: method.to_string(),
            params,
        };
    };

    match kind {
        EventKind::AssistantStream => try_parse(method, params, Notification::AssistantStream),
        EventKind::AssistantMessage => try_parse(method, params, Notification::AssistantMessage),
        EventKind::ToolCall => try_parse(method, params, Notification::ToolCall),
        EventKind::ToolResult => try_parse(method, params, Notification::ToolResult),
        EventKind::ToolProgress => try_parse(method, params, Notification::ToolProgress),
        EventKind::PromptRequest => try_parse(method, params, Notification::PromptRequest),
        EventKind::ConnectorReply => try_parse(method, params, Notification::ConnectorReply),
        EventKind::AgentCancelled => try_parse(method, params, Notification::AgentCancelled),
        EventKind::AgentYielded => try_parse(method, params, Notification::AgentYielded),
        EventKind::Error => try_parse(method, params, Notification::Error),
        _ => Notification::Unknown {
            method: method.to_string(),
            params,
        },
    }
}

/// Deserialize params into `T` and wrap with `wrap`, falling back to `Unknown`.
fn try_parse<T, F>(method: &str, params: serde_json::Value, wrap: F) -> Notification
where
    T: serde::de::DeserializeOwned,
    F: FnOnce(T) -> Notification,
{
    match serde_json::from_value::<T>(params.clone()) {
        Ok(event) => wrap(event),
        Err(_) => Notification::Unknown {
            method: method.to_string(),
            params,
        },
    }
}
