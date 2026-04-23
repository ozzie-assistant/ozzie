use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite;
use tracing::warn;

use ozzie_protocol::{Frame, PromptResponseParams};

/// A submitted device pairing request.
#[derive(Debug)]
pub struct PairingRequest {
    pub request_id: String,
    pub expires_at: String,
}

/// Result of polling for a device pairing request.
#[derive(Debug)]
pub enum PairingStatus {
    Pending,
    Approved { device_id: String, token: String },
    Rejected,
}

/// Options for opening a session.
pub struct OpenConversationOpts<'a> {
    pub conversation_id: Option<&'a str>,
    pub working_dir: Option<&'a str>,
}

/// Errors from the WebSocket client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("connection closed")]
    Closed,
    #[error("server error: {0}")]
    Server(String),
    #[error("timeout")]
    Timeout,
    #[error("{0}")]
    Other(String),
}

impl From<tungstenite::Error> for ClientError {
    fn from(e: tungstenite::Error) -> Self {
        Self::WebSocket(e.to_string())
    }
}

/// WebSocket client for connecting to the Ozzie gateway.
pub struct OzzieClient {
    sink: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tungstenite::Message,
    >,
    stream: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
    req_counter: AtomicU64,
    conversation_id: Option<String>,
    /// Buffer for frames received during request() that weren't the expected response.
    pending_frames: Vec<Frame>,
}

impl OzzieClient {
    /// Default gateway WebSocket URL.
    pub const DEFAULT_WS_URL: &str = "ws://127.0.0.1:18420/api/ws";

    /// Discovers the default gateway URL.
    pub fn discover_gateway_url() -> String {
        Self::DEFAULT_WS_URL.to_string()
    }

    /// Discovers the auth token from $OZZIE_PATH/.token or ~/.ozzie/.token.
    pub fn discover_token() -> Option<String> {
        let ozzie_path = std::env::var("OZZIE_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                dirs_fallback().join(".ozzie")
            });
        let path = ozzie_path.join(".token");
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Reads the device key from `dir/.key`, generating and persisting it if absent.
    pub fn read_or_generate_key(dir: &std::path::Path) -> String {
        let path = dir.join(".key");
        if let Ok(s) = std::fs::read_to_string(&path) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
        let key = uuid::Uuid::new_v4().to_string();
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!(error = %e, "failed to create key directory");
        }
        if let Err(e) = std::fs::write(&path, &key) {
            warn!(error = %e, "failed to persist device key");
        }
        key
    }

    /// Connects to the gateway WebSocket endpoint.
    pub async fn connect(url: &str, token: Option<&str>) -> Result<Self, ClientError> {
        let ws_url = if url.starts_with("http") {
            url.replacen("http", "ws", 1)
        } else {
            url.to_string()
        };

        let full_url = if ws_url.ends_with("/api/ws") {
            ws_url
        } else {
            format!("{}/api/ws", ws_url.trim_end_matches('/'))
        };

        let full_url = if let Some(t) = token {
            if full_url.contains('?') {
                format!("{full_url}&token={t}")
            } else {
                format!("{full_url}?token={t}")
            }
        } else {
            full_url
        };

        let (ws_stream, _) = tokio_tungstenite::connect_async(&full_url)
            .await
            .map_err(|e| ClientError::WebSocket(format!("connect: {e}")))?;

        let (sink, stream) = ws_stream.split();

        Ok(Self {
            sink,
            stream,
            req_counter: AtomicU64::new(1),
            conversation_id: None,
            pending_frames: Vec::new(),
        })
    }

    /// Returns the current session ID if one has been opened.
    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// Opens or resumes a session.
    pub async fn open_session(&mut self, opts: OpenConversationOpts<'_>) -> Result<String, ClientError> {
        let mut params = serde_json::json!({});
        if let Some(sid) = opts.conversation_id {
            params["conversation_id"] = serde_json::json!(sid);
        }
        if let Some(dir) = opts.working_dir {
            params["working_dir"] = serde_json::json!(dir);
        }

        let resp = self.request("open_conversation", params).await?;
        let sid = resp
            .result
            .as_ref()
            .and_then(|p| p.get("conversation_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        self.conversation_id = Some(sid.clone());
        Ok(sid)
    }

    /// Sends a text message in the current session.
    pub async fn send_message(&mut self, text: &str) -> Result<(), ClientError> {
        let conversation_id = self
            .conversation_id
            .as_deref()
            .ok_or_else(|| ClientError::Other("no session open".to_string()))?;
        let params = serde_json::json!({ "conversation_id": conversation_id, "text": text });
        self.request("send_message", params).await?;
        Ok(())
    }

    /// Sends a text message with image attachments in the current session.
    pub async fn send_message_with_images(
        &mut self,
        text: &str,
        images: Vec<ozzie_types::ImageAttachment>,
    ) -> Result<(), ClientError> {
        let conversation_id = self
            .conversation_id
            .as_deref()
            .ok_or_else(|| ClientError::Other("no session open".to_string()))?;
        let params = serde_json::json!({
            "conversation_id": conversation_id,
            "text": text,
            "images": images,
        });
        self.request("send_message", params).await?;
        Ok(())
    }

    /// Sends a connector message to the gateway with full identity metadata.
    ///
    /// Unlike `send_message()`, this publishes a `ConnectorMessage` event that
    /// goes through identity resolution, pairing checks, and connector routing.
    pub async fn send_connector_message(
        &mut self,
        connector: &str,
        channel_id: &str,
        author: &str,
        content: &str,
        message_id: Option<&str>,
        server_id: Option<&str>,
    ) -> Result<(), ClientError> {
        let mut params = serde_json::json!({
            "connector": connector,
            "channel_id": channel_id,
            "author": author,
            "content": content,
        });
        if let Some(mid) = message_id {
            params["message_id"] = serde_json::json!(mid);
        }
        if let Some(sid) = server_id {
            params["server_id"] = serde_json::json!(sid);
        }
        self.request("send_connector_message", params).await?;
        Ok(())
    }

    /// Reads the next frame from the server.
    /// Returns buffered frames first (from request() overflow), then reads from WS.
    pub async fn read_frame(&mut self) -> Result<Frame, ClientError> {
        if !self.pending_frames.is_empty() {
            return Ok(self.pending_frames.remove(0));
        }
        self.read_frame_from_ws().await
    }

    /// Responds to a prompt request with typed parameters.
    pub async fn respond_to_prompt(
        &mut self,
        params: PromptResponseParams,
    ) -> Result<(), ClientError> {
        let value = serde_json::to_value(&params)
            .map_err(|e| ClientError::Other(format!("serialize prompt response: {e}")))?;
        self.request("prompt_response", value).await?;
        Ok(())
    }

    /// Accepts all dangerous tools for the current session.
    pub async fn accept_all_tools(&mut self) -> Result<(), ClientError> {
        let conversation_id = self
            .conversation_id
            .as_deref()
            .ok_or_else(|| ClientError::Other("no session open".to_string()))?;
        self.request(
            "accept_all_tools",
            serde_json::json!({ "conversation_id": conversation_id }),
        )
        .await?;
        Ok(())
    }

    /// Loads message history.
    pub async fn load_messages(
        &mut self,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>, ClientError> {
        let conversation_id = self
            .conversation_id
            .as_deref()
            .ok_or_else(|| ClientError::Other("no session open".to_string()))?;
        let mut params = serde_json::json!({ "conversation_id": conversation_id });
        if let Some(n) = limit {
            params["limit"] = serde_json::json!(n);
        }

        let resp = self.request("load_messages", params).await?;
        let messages = resp
            .result
            .as_ref()
            .and_then(|p| p.get("messages"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(messages)
    }

    /// Submits a device pairing request to the gateway via HTTP.
    pub async fn request_pairing(
        gateway_http_url: &str,
        client_type: &str,
        label: Option<&str>,
        device_key: Option<&str>,
    ) -> Result<PairingRequest, ClientError> {
        let url = format!("{}/api/pair", gateway_http_url.trim_end_matches('/'));
        let mut body = serde_json::json!({ "client_type": client_type });
        if let Some(l) = label {
            body["label"] = serde_json::json!(l);
        }
        if let Some(k) = device_key {
            body["device_key"] = serde_json::json!(k);
        }

        let resp = reqwest::Client::new()
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("pair request: {e}")))?;

        if !resp.status().is_success() {
            return Err(ClientError::Other(format!(
                "pair request failed: HTTP {}",
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("parse pair response: {e}")))?;

        Ok(PairingRequest {
            request_id: json["request_id"].as_str().unwrap_or("").to_string(),
            expires_at: json["expires_at"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Polls the gateway for the status of a pending device pairing request.
    pub async fn poll_pairing(
        gateway_http_url: &str,
        request_id: &str,
    ) -> Result<PairingStatus, ClientError> {
        let url = format!(
            "{}/api/pair/{request_id}",
            gateway_http_url.trim_end_matches('/')
        );

        let resp = reqwest::get(&url)
            .await
            .map_err(|e| ClientError::Other(format!("poll pairing: {e}")))?;

        if !resp.status().is_success() {
            return Err(ClientError::Other(format!(
                "poll failed: HTTP {}",
                resp.status()
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("parse poll response: {e}")))?;

        match json["status"].as_str().unwrap_or("pending") {
            "approved" => Ok(PairingStatus::Approved {
                device_id: json["device_id"].as_str().unwrap_or("").to_string(),
                token: json["token"].as_str().unwrap_or("").to_string(),
            }),
            "rejected" => Ok(PairingStatus::Rejected),
            _ => Ok(PairingStatus::Pending),
        }
    }

    /// Obtains a valid bearer token for the target gateway (CLI mode).
    pub async fn acquire_token_cli(
        gateway_url: &str,
        ozzie_path: &std::path::Path,
    ) -> Result<String, ClientError> {
        use crate::credential::{Credential, CredentialStore, FileCredentialStore};

        let device_key = Self::read_or_generate_key(ozzie_path);
        let cred_store = FileCredentialStore::new(ozzie_path.join(".credential.json"));

        if let Ok(Some(cred)) = cred_store.load(gateway_url) {
            return Ok(cred.token);
        }

        let hostname = std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "cli".to_string());

        eprintln!("Pairing with gateway {gateway_url}...");

        let pair_req =
            Self::request_pairing(gateway_url, "cli", Some(&hostname), Some(&device_key)).await?;

        eprintln!(
            "Waiting for pairing approval (request: {})...",
            &pair_req.request_id[..8.min(pair_req.request_id.len())]
        );
        eprintln!("Run: ozzie pairing requests approve {}", pair_req.request_id);

        let mut elapsed_secs: u64 = 0;
        loop {
            match Self::poll_pairing(gateway_url, &pair_req.request_id).await {
                Ok(PairingStatus::Approved { device_id, token }) => {
                    let cred = Credential {
                        device_id,
                        token: token.clone(),
                        gateway_url: gateway_url.to_string(),
                    };
                    if let Err(e) = cred_store.save(&cred) {
                        warn!(error = %e, "failed to save credential");
                    }
                    eprintln!("\nPairing approved!");
                    return Ok(token);
                }
                Ok(PairingStatus::Rejected) => {
                    return Err(ClientError::Other(
                        "pairing rejected by gateway".to_string(),
                    ));
                }
                Ok(PairingStatus::Pending) | Err(_) => {}
            }

            elapsed_secs += 2;
            eprint!("\rWaiting... {elapsed_secs}s  ");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    /// Closes the WebSocket connection.
    pub async fn close(&mut self) -> Result<(), ClientError> {
        self.sink
            .send(tungstenite::Message::Close(None))
            .await
            .map_err(|e| ClientError::WebSocket(format!("close: {e}")))?;
        Ok(())
    }

    /// Reads a frame directly from the WebSocket stream (bypasses pending buffer).
    async fn read_frame_from_ws(&mut self) -> Result<Frame, ClientError> {
        loop {
            match self.stream.next().await {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    let frame: Frame = serde_json::from_str(&text)
                        .map_err(|e| ClientError::Other(format!("parse frame: {e}")))?;
                    return Ok(frame);
                }
                Some(Ok(tungstenite::Message::Binary(data))) => {
                    let frame = Frame::from_bytes(&data)
                        .map_err(|e| ClientError::Other(format!("parse frame: {e}")))?;
                    return Ok(frame);
                }
                Some(Ok(tungstenite::Message::Ping(_))) => continue,
                Some(Ok(tungstenite::Message::Pong(_))) => continue,
                Some(Ok(tungstenite::Message::Close(_))) => return Err(ClientError::Closed),
                Some(Err(e)) => return Err(ClientError::WebSocket(e.to_string())),
                None => return Err(ClientError::Closed),
                _ => continue,
            }
        }
    }

    /// Sends a request and waits for the matching response.
    async fn request(
        &mut self,
        method_name: &str,
        params: serde_json::Value,
    ) -> Result<Frame, ClientError> {
        let req_id = format!("req_{}", self.req_counter.fetch_add(1, Ordering::Relaxed));
        let frame = Frame::request(&req_id, method_name, &params);

        let bytes = frame
            .to_bytes()
            .map_err(|e| ClientError::Other(format!("serialize: {e}")))?;
        self.sink
            .send(tungstenite::Message::Binary(bytes.into()))
            .await
            .map_err(|e| ClientError::WebSocket(format!("send: {e}")))?;

        // Wait for matching response
        let timeout = tokio::time::Duration::from_secs(30);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ClientError::Timeout);
            }

            match tokio::time::timeout(remaining, self.read_frame_from_ws()).await {
                Ok(Ok(resp)) => {
                    if resp.is_response() && resp.id.as_deref() == Some(&req_id) {
                        if resp.is_error() {
                            return Err(ClientError::Server(
                                resp.error_message()
                                    .unwrap_or("unknown error")
                                    .to_string(),
                            ));
                        }
                        return Ok(resp);
                    }
                    // Not our response — buffer it for later read_frame() calls
                    self.pending_frames.push(resp);
                    continue;
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(ClientError::Timeout),
            }
        }
    }
}

/// Fallback home directory resolution (no dependency on ozzie-core).
fn dirs_fallback() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_token_missing() {
        let _ = OzzieClient::discover_token();
    }

    #[test]
    fn frame_request_id_increments() {
        let counter = AtomicU64::new(1);
        let id1 = format!("req_{}", counter.fetch_add(1, Ordering::Relaxed));
        let id2 = format!("req_{}", counter.fetch_add(1, Ordering::Relaxed));
        assert_eq!(id1, "req_1");
        assert_eq!(id2, "req_2");
    }

    #[tokio::test]
    async fn connect_fails_on_invalid_url() {
        let result = OzzieClient::connect("ws://127.0.0.1:1", None).await;
        assert!(result.is_err());
    }

    #[test]
    fn default_gateway_url() {
        let url = OzzieClient::discover_gateway_url();
        assert_eq!(url, "ws://127.0.0.1:18420/api/ws");
    }
}
