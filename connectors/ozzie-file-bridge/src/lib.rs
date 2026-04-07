use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

use ozzie_client::{ClientError, EventKind, OzzieClient, OpenSessionOpts, PromptResponseParams};

/// Configuration as deserialized from `config.connectors.file`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConnectorConfig {
    /// Set to false to temporarily disable the connector without removing config.
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// Path to the input JSONL file (one JSON message per line).
    pub input: String,
    /// Path to the output JSONL file (responses appended).
    pub output: String,
    /// Policy name for auto-pairing (unused in JSON-RPC mode, kept for compat).
    #[serde(default = "default_auto_pair_policy")]
    pub auto_pair_policy: String,
}

impl Default for FileConnectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            input: String::new(),
            output: String::new(),
            auto_pair_policy: default_auto_pair_policy(),
        }
    }
}

fn bool_true() -> bool {
    true
}

fn default_auto_pair_policy() -> String {
    "admin".to_string()
}

/// Input message format (one per line in input JSONL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    /// Channel identifier (for session routing).
    #[serde(default = "default_channel")]
    pub channel_id: String,
    /// Author name (informational).
    #[serde(default = "default_author")]
    pub author: String,
    /// Message text.
    pub content: String,
}

fn default_channel() -> String {
    "file".to_string()
}

fn default_author() -> String {
    "user".to_string()
}

/// Output message format (one per line in output JSONL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMessage {
    pub channel_id: String,
    pub content: String,
}

/// File connector bridge — polls input JSONL, sends to gateway via JSON-RPC,
/// writes responses to output JSONL.
///
/// This is a standalone bridge, not an in-process connector. It connects to
/// the Ozzie gateway as a regular JSON-RPC WebSocket client.
pub struct FileBridge {
    config: FileConnectorConfig,
}

impl FileBridge {
    pub fn new(config: FileConnectorConfig) -> Result<Self, String> {
        if config.input.is_empty() {
            return Err("file connector: input path is required".to_string());
        }
        if config.output.is_empty() {
            return Err("file connector: output path is required".to_string());
        }
        Ok(Self { config })
    }

    /// Creates a bridge from `OZZIE_CONNECTOR_CONFIG` environment variable.
    ///
    /// Used when launched by the ProcessSupervisor.
    pub fn from_env() -> Result<Self, String> {
        let json = std::env::var("OZZIE_CONNECTOR_CONFIG")
            .map_err(|_| "OZZIE_CONNECTOR_CONFIG not set".to_string())?;
        let config: FileConnectorConfig =
            serde_json::from_str(&json).map_err(|e| format!("invalid OZZIE_CONNECTOR_CONFIG: {e}"))?;
        Self::new(config)
    }

    /// Runs the bridge loop. Connects to the gateway, polls input, writes output.
    ///
    /// This method blocks until the gateway connection is closed or an
    /// unrecoverable error occurs.
    ///
    /// Falls back to `OZZIE_GATEWAY_URL` and `OZZIE_GATEWAY_TOKEN` env vars
    /// when the corresponding arguments are not provided.
    pub async fn run(&self, gateway_url: &str, token: Option<&str>) -> Result<(), String> {
        let gateway_url = if gateway_url.is_empty() {
            std::env::var("OZZIE_GATEWAY_URL")
                .unwrap_or_else(|_| "ws://127.0.0.1:18420/ws".to_string())
        } else {
            gateway_url.to_string()
        };
        let token_env = std::env::var("OZZIE_GATEWAY_TOKEN").ok();
        let token = token.map(String::from).or(token_env);

        let mut client = OzzieClient::connect(&gateway_url, token.as_deref())
            .await
            .map_err(|e| format!("file connector: connect: {e}"))?;

        // Open a dedicated session for this connector
        let session_id = client
            .open_session(OpenSessionOpts {
                session_id: None,
                working_dir: None,
            })
            .await
            .map_err(|e| format!("file connector: open session: {e}"))?;

        // Accept all tools (file connector is trusted)
        if let Err(e) = client.accept_all_tools().await {
            warn!(error = %e, "file connector: failed to accept all tools");
        }

        info!(
            session_id = %session_id,
            input = %self.config.input,
            output = %self.config.output,
            "file connector bridge started"
        );

        // Ensure input file exists
        if !tokio::fs::try_exists(&self.config.input).await.unwrap_or(false) {
            if let Some(parent) = std::path::Path::new(&self.config.input).parent()
                && let Err(e) = tokio::fs::create_dir_all(parent).await
            {
                warn!(error = %e, "file connector: failed to create input parent dir");
            }
            if let Err(e) = tokio::fs::File::create(&self.config.input).await {
                warn!(error = %e, "file connector: failed to create input file");
            }
        }

        // Main loop: poll input file, send to gateway, write responses
        loop {
            // Poll for input messages
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let content = match tokio::fs::read_to_string(&self.config.input).await {
                Ok(c) if c.trim().is_empty() => {
                    // No messages — check for pending events from gateway
                    self.drain_events(&mut client).await;
                    continue;
                }
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "file connector: read error, retrying");
                    continue;
                }
            };

            // Truncate immediately
            if let Err(e) = tokio::fs::write(&self.config.input, "").await {
                warn!(error = %e, "file connector: truncate error");
            }

            // Process each line
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                let msg: InputMessage = match serde_json::from_str(trimmed) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(error = %e, "file connector: failed to parse input message");
                        continue;
                    }
                };

                debug!(author = %msg.author, "file connector: sending connector message");

                // Send to gateway as a connector message (with identity metadata)
                if let Err(e) = client
                    .send_connector_message(
                        "file",
                        &msg.channel_id,
                        &msg.author,
                        &msg.content,
                        None,
                    )
                    .await
                {
                    warn!(error = %e, "file connector: send_connector_message failed");
                    continue;
                }

                // Wait for assistant response
                match self.wait_for_response(&mut client, &msg.channel_id).await {
                    Ok(()) => {}
                    Err(e) => {
                        warn!(error = %e, "file connector: response error");
                    }
                }
            }
        }
    }

    /// Waits for the assistant's complete response and writes it to output.
    async fn wait_for_response(
        &self,
        client: &mut OzzieClient,
        channel_id: &str,
    ) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("timeout waiting for response".to_string());
            }

            match tokio::time::timeout(remaining, client.read_frame()).await {
                Ok(Ok(frame)) => {
                    if !frame.is_notification() {
                        continue;
                    }

                    match frame.event_kind() {
                        Some(EventKind::AssistantMessage) => {
                            let content = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("content"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            if !content.is_empty() {
                                self.write_output(channel_id, &content).await?;
                            }
                            return Ok(());
                        }
                        Some(EventKind::PromptRequest) => {
                            // Auto-approve all prompts
                            if let Some(token) = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("token"))
                                .and_then(|v| v.as_str())
                                && let Err(e) = client
                                    .respond_to_prompt(PromptResponseParams {
                                        token: token.to_string(),
                                        value: Some("session".to_string()),
                                        text: None,
                                    })
                                    .await
                            {
                                warn!(error = %e, "file connector: failed to respond to prompt");
                            }
                        }
                        Some(EventKind::Error) => {
                            let msg = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error");
                            return Err(format!("agent error: {msg}"));
                        }
                        _ => {} // ignore other events
                    }
                }
                Ok(Err(ClientError::Closed)) => {
                    return Err("connection closed".to_string());
                }
                Ok(Err(e)) => {
                    return Err(format!("read error: {e}"));
                }
                Err(_) => {
                    return Err("timeout".to_string());
                }
            }
        }
    }

    /// Drains pending events from the gateway (non-blocking).
    async fn drain_events(&self, client: &mut OzzieClient) {
        while let Ok(Ok(_frame)) = tokio::time::timeout(
            tokio::time::Duration::from_millis(10),
            client.read_frame(),
        )
        .await
        {
            // Discard — no pending input to respond to
        }
    }

    async fn write_output(&self, channel_id: &str, content: &str) -> Result<(), String> {
        let msg = OutputMessage {
            channel_id: channel_id.to_string(),
            content: content.to_string(),
        };
        let json = serde_json::to_string(&msg)
            .map_err(|e| format!("serialize output: {e}"))?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.output)
            .await
            .map_err(|e| format!("open output {}: {e}", self.config.output))?;

        file.write_all(json.as_bytes())
            .await
            .map_err(|e| format!("write output: {e}"))?;
        file.write_all(b"\n")
            .await
            .map_err(|e| format!("write newline: {e}"))?;
        file.flush()
            .await
            .map_err(|e| format!("flush output: {e}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_empty_input() {
        let err = FileBridge::new(FileConnectorConfig {
            input: String::new(),
            output: "/tmp/out.jsonl".to_string(),
            ..Default::default()
        });
        assert!(err.is_err());
    }

    #[test]
    fn new_rejects_empty_output() {
        let err = FileBridge::new(FileConnectorConfig {
            input: "/tmp/in.jsonl".to_string(),
            output: String::new(),
            ..Default::default()
        });
        assert!(err.is_err());
    }

    #[test]
    fn input_message_parse() {
        let json = r#"{"channel_id":"bench","author":"user","content":"Hello"}"#;
        let msg: InputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.content, "Hello");
        assert_eq!(msg.channel_id, "bench");
    }

    #[test]
    fn input_message_minimal() {
        let json = r#"{"content":"Hello"}"#;
        let msg: InputMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.content, "Hello");
        assert_eq!(msg.channel_id, "file"); // default
        assert_eq!(msg.author, "user"); // default
    }

    #[test]
    fn output_message_roundtrip() {
        let msg = OutputMessage {
            channel_id: "ch_1".to_string(),
            content: "hello from ozzie".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: OutputMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "hello from ozzie");
    }
}
