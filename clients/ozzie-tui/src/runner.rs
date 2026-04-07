use std::path::PathBuf;

use ozzie_client::{
    ClientError, ClientEvent, Credential, CredentialStore, EventKind, FileCredentialStore,
    OpenSessionOpts, OzzieClient, PairingStatus, PromptRequestPayload, PromptResponseParams,
};
use tokio::sync::mpsc;

use crate::app::{App, ConnectionState};
use crate::backend::{UiBackend, UiEvent};
use crate::render::RenderContext;

/// Commands sent from the main loop to the background WS task.
enum WsCommand {
    SendMessage(String),
    RespondPrompt {
        token: String,
        response: String,
    },
    Shutdown,
}

/// Configuration for `run_ui`.
pub struct RunConfig {
    /// HTTP base URL of the gateway (e.g. `http://127.0.0.1:18420`).
    pub gateway_url: String,
    /// Local Ozzie installation path used for `.key` and `.credential.json`.
    pub ozzie_path: PathBuf,
    pub session_id: Option<String>,
    pub working_dir: Option<String>,
    pub render_ctx: RenderContext,
}

/// Generic UI event loop — drives any `UiBackend`.
///
/// Handles WS protocol, app state, and delegates rendering to the backend.
pub async fn run_ui<B: UiBackend>(
    backend: &mut B,
    config: RunConfig,
) -> anyhow::Result<()> {
    let mut app = App::new();
    app.render_ctx = config.render_ctx.clone();

    backend.setup()?;

    // Ensure teardown runs even on error / panic
    let result = run_loop(backend, &mut app, &config).await;

    backend.teardown()?;
    result
}

/// Obtains a valid bearer token for the target gateway.
///
/// Flow:
/// 1. Read (or generate) `$ozzie_path/.key` — the device fingerprint.
/// 2. Check `$ozzie_path/.credential.json` for a stored token for this gateway URL.
/// 3. If found → return it directly (will be validated by the gateway on connect;
///    a 401 would require a fresh start, but we don't retry here).
/// 4. If not found → POST `/api/pair` with the device key.
///    - If the gateway shares the same key (same OZZIE_PATH), the first poll
///      returns `approved` immediately (auto-approve).
///    - Otherwise → show "Waiting for pairing approval..." and poll every 2 s.
/// 5. On approval → persist credential and return the token.
/// 6. Returns `None` if the user quits during the wait.
async fn acquire_token<B: UiBackend>(
    backend: &mut B,
    app: &mut App,
    config: &RunConfig,
) -> anyhow::Result<Option<String>> {
    let device_key = OzzieClient::read_or_generate_key(&config.ozzie_path);

    let cred_store = FileCredentialStore::new(config.ozzie_path.join(".credential.json"));

    // Return stored credential if available.
    if let Ok(Some(cred)) = cred_store.load(&config.gateway_url) {
        return Ok(Some(cred.token));
    }

    // No stored credential — start pairing flow.
    app.status = "Pairing with gateway...".to_string();
    backend.render(app)?;

    let hostname = hostname_label();
    let pair_req = OzzieClient::request_pairing(
        &config.gateway_url,
        "tui",
        Some(&hostname),
        Some(&device_key),
    )
    .await
    .map_err(|e| anyhow::anyhow!("pairing request failed: {e}"))?;

    // Poll until approved, rejected, or user quits.
    let mut elapsed_secs: u64 = 0;
    loop {
        match OzzieClient::poll_pairing(&config.gateway_url, &pair_req.request_id).await {
            Ok(PairingStatus::Approved { device_id, token }) => {
                // Persist credential for future sessions.
                let cred = Credential {
                    device_id,
                    token: token.clone(),
                    gateway_url: config.gateway_url.clone(),
                };
                let _ = cred_store.save(&cred);
                return Ok(Some(token));
            }
            Ok(PairingStatus::Rejected) => {
                app.status = "Pairing rejected by gateway.".to_string();
                app.connection = ConnectionState::Disconnected;
                backend.render(app)?;
                loop {
                    if let Some(UiEvent::Quit) = backend.next_event().await {
                        return Ok(None);
                    }
                }
            }
            Ok(PairingStatus::Pending) | Err(_) => {}
        }

        elapsed_secs += 2;
        app.status = format!(
            "Waiting for pairing approval... {}s  (request: {})",
            elapsed_secs, &pair_req.request_id[..8]
        );
        backend.render(app)?;

        // Check for quit while waiting.
        if let Ok(Some(UiEvent::Quit)) = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            backend.next_event(),
        )
        .await
        {
            return Ok(None);
        }
    }
}

/// Returns a human-readable label for this machine (hostname or fallback).
fn hostname_label() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "tui".to_string())
}

async fn run_loop<B: UiBackend>(
    backend: &mut B,
    app: &mut App,
    config: &RunConfig,
) -> anyhow::Result<()> {
    app.status = "Connecting...".to_string();
    app.connection = ConnectionState::Connecting;
    backend.render(app)?;

    // Acquire a valid token via the credential store + device pairing flow.
    let token = match acquire_token(backend, app, config).await? {
        Some(t) => t,
        None => return Ok(()), // user quit during pairing
    };

    // Connect to gateway WebSocket
    let client = match OzzieClient::connect(&config.gateway_url, Some(&token)).await {
        Ok(c) => c,
        Err(e) => {
            app.connection = ConnectionState::Disconnected;
            app.status = format!("Connection failed: {e}");
            backend.render(app)?;
            loop {
                if let Some(UiEvent::Quit) = backend.next_event().await {
                    return Ok(());
                }
            }
        }
    };

    // Open session
    let mut session_client = client;
    let session_id = session_client
        .open_session(OpenSessionOpts {
            session_id: config.session_id.as_deref(),
            working_dir: config.working_dir.as_deref(),
        })
        .await?;
    app.session_id = Some(session_id);
    app.connection = ConnectionState::Connected;
    app.status = "Ready".to_string();

    // Load session history
    match session_client.load_messages(Some(50)).await {
        Ok(messages) => {
            for msg in &messages {
                let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match role {
                    "user" => app.push_user_message(content),
                    "assistant" => {
                        app.start_assistant();
                        app.append_stream(content);
                        app.finalize_assistant();
                    }
                    "tool" => {
                        let tool_name = msg
                            .get("tool_call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        app.add_history_tool(tool_name, content);
                    }
                    _ => {}
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "failed to load history"),
    }

    // Start WS read+write loop in background with command channel
    let (ws_event_tx, mut ws_event_rx) = mpsc::unbounded_channel();
    let (ws_cmd_tx, ws_cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(run_ws_loop(session_client, ws_event_tx, ws_cmd_rx));

    loop {
        // 1. Flush finalized blocks
        let finalized = app.drain_finalized();
        if !finalized.is_empty() {
            backend.flush_finalized(&finalized, &app.render_ctx)?;
        }

        // 2. Render
        backend.render(app)?;

        // 3. Wait for next event
        tokio::select! {
            event = backend.next_event() => {
                match event {
                    Some(UiEvent::Quit) => {
                        let _ = ws_cmd_tx.send(WsCommand::Shutdown);
                        app.should_quit = true;
                        break;
                    }
                    Some(UiEvent::SendMessage(_)) => {
                        handle_enter(app, &ws_cmd_tx);
                    }
                    Some(ev) => apply_ui_event(ev, app),
                    None => break, // event source closed
                }
            }
            Some(ws_event) = ws_event_rx.recv() => {
                apply_ws_event(ws_event, app);
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handles Enter key — either confirms a prompt, toggles collapse, or sends a message.
fn handle_enter(app: &mut App, ws_cmd_tx: &mpsc::UnboundedSender<WsCommand>) {
    // Prompt mode: confirm selected option
    if let Some(prompt) = app.active_prompt.take() {
        let response = prompt.response_value().to_string();
        let _ = ws_cmd_tx.send(WsCommand::RespondPrompt {
            token: prompt.token,
            response,
        });
        app.status = "Ready".to_string();
        return;
    }

    // Block selection mode: toggle collapse
    if app.selected_block.is_some() {
        app.toggle_selected_collapse();
        return;
    }

    // Normal mode: send message
    let text = app.input.take_input();
    if !text.is_empty() {
        app.push_user_message(&text);
        app.scroll_offset = 0;
        let _ = ws_cmd_tx.send(WsCommand::SendMessage(text));
    }
}

/// Maps a UI event to App state mutations.
fn apply_ui_event(ev: UiEvent, app: &mut App) {
    match ev {
        // Up/Down: navigate within multiline input, or scroll
        UiEvent::ScrollUp => {
            if !app.has_prompt() && app.input.is_multiline() && !app.input.cursor_on_first_line() {
                app.input.move_up();
            } else {
                app.scroll_up(1);
            }
        }
        UiEvent::ScrollDown => {
            if !app.has_prompt() && app.input.is_multiline() && !app.input.cursor_on_last_line() {
                app.input.move_down();
            } else {
                app.scroll_down(1);
            }
        }
        UiEvent::PageUp => app.scroll_up(10),
        UiEvent::PageDown => app.scroll_down(10),
        // Left/Right: prompt navigation when prompt active, otherwise input cursor
        UiEvent::PromptLeft => {
            if let Some(ref mut p) = app.active_prompt {
                p.select_prev();
            } else {
                app.input.move_left();
            }
        }
        UiEvent::PromptRight => {
            if let Some(ref mut p) = app.active_prompt {
                p.select_next();
            } else {
                app.input.move_right();
            }
        }
        UiEvent::PromptConfirm => {} // handled via SendMessage/Enter
        // Block navigation
        UiEvent::ToggleCollapse => app.toggle_selected_collapse(),
        UiEvent::SelectNextTool => app.select_next_tool(),
        UiEvent::Deselect => app.deselect(),
        // Input
        UiEvent::InputChar(c) => {
            if !app.has_prompt() {
                app.input.insert_char(c);
            }
        }
        UiEvent::InputBackspace => {
            if !app.has_prompt() {
                app.input.backspace();
            }
        }
        UiEvent::InputDelete => {
            if !app.has_prompt() {
                app.input.delete();
            }
        }
        UiEvent::InputLeft => {
            if !app.has_prompt() {
                app.input.move_left();
            }
        }
        UiEvent::InputRight => {
            if !app.has_prompt() {
                app.input.move_right();
            }
        }
        UiEvent::InputHome => {
            if !app.has_prompt() {
                app.input.home();
            }
        }
        UiEvent::InputEnd => {
            if !app.has_prompt() {
                app.input.end();
            }
        }
        UiEvent::InputClearLine => {
            if !app.has_prompt() {
                app.input.clear_line();
            }
        }
        UiEvent::InputNewline => {
            if !app.has_prompt() {
                app.input.insert_newline();
            }
        }
        UiEvent::Paste(text) => {
            if !app.has_prompt() {
                app.input.insert_str(&text);
            }
        }
        UiEvent::Tick => {
            app.advance_spinner();
        }
        UiEvent::Quit | UiEvent::SendMessage(_) => {}
    }
}

/// Maps a WebSocket event to App state mutations.
fn apply_ws_event(ev: ClientEvent, app: &mut App) {
    match ev {
        ClientEvent::Connected { session_id } => {
            app.session_id = Some(session_id);
            app.connection = ConnectionState::Connected;
            app.status = "Connected".to_string();
        }
        ClientEvent::StreamDelta { content } => {
            app.finalize_pending_tools();
            app.start_working();
            app.append_stream(&content);
        }
        ClientEvent::MessageComplete { .. } => {
            app.finalize_pending_tools();
            app.finalize_assistant();
            app.stop_working();
        }
        ClientEvent::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            app.start_working();
            app.ensure_assistant();
            app.add_tool_call(&call_id, &name, &arguments);
            app.status = format!("Tool: {name}");
        }
        ClientEvent::ToolResult {
            call_id,
            result,
            is_error,
        } => {
            app.set_tool_result(&call_id, &result, is_error);
        }
        ClientEvent::PromptRequest {
            token,
            prompt_type,
            label,
        } => {
            app.set_prompt(token, label.clone(), prompt_type);
            app.status = format!("⚠ Approve: {label}");
        }
        ClientEvent::SkillEvent {
            event_type, skill, ..
        } => {
            app.status = format!("[{event_type}: {skill}]");
        }
        ClientEvent::Error { message } => {
            app.add_system_message(&message);
            app.status = format!("Error: {message}");
        }
        ClientEvent::Disconnected => {
            app.connection = ConnectionState::Disconnected;
            app.status = "Disconnected".to_string();
        }
    }
}

/// Background task that owns the WS connection.
/// Handles both reading frames and executing write commands.
async fn run_ws_loop(
    mut client: OzzieClient,
    event_tx: mpsc::UnboundedSender<ClientEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<WsCommand>,
) {
    tracing::debug!("ws loop started");
    loop {
        tokio::select! {
            frame_result = client.read_frame() => {
                match frame_result {
                    Ok(frame) => {
                        if !frame.is_notification() {
                            tracing::trace!("non-notification frame, skipping");
                            continue;
                        }
                        let event_type = frame.method.as_deref().unwrap_or("?");
                        match parse_frame(&frame) {
                            Some(ev) => {
                                tracing::debug!(event_type, "ws event parsed");
                                if event_tx.send(ev).is_err() {
                                    tracing::warn!("event channel closed");
                                    break;
                                }
                            }
                            None => {
                                tracing::trace!(event_type, "ws event ignored");
                            }
                        }
                    }
                    Err(ClientError::Closed) => {
                        tracing::info!("ws connection closed");
                        let _ = event_tx.send(ClientEvent::Disconnected);
                        break;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "ws read error");
                        let _ = event_tx.send(ClientEvent::Error {
                            message: e.to_string(),
                        });
                        let _ = event_tx.send(ClientEvent::Disconnected);
                        break;
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WsCommand::SendMessage(text)) => {
                        tracing::debug!(len = text.len(), "sending message");
                        if let Err(e) = client.send_message(&text).await {
                            tracing::error!(error = %e, "send failed");
                            let _ = event_tx.send(ClientEvent::Error {
                                message: format!("Send failed: {e}"),
                            });
                        }
                    }
                    Some(WsCommand::RespondPrompt { token, response }) => {
                        let params = PromptResponseParams {
                            token: token.clone(),
                            value: Some(response),
                            text: None,
                        };
                        if let Err(e) = client.respond_to_prompt(params).await {
                            let _ = event_tx.send(ClientEvent::Error {
                                message: format!("Prompt response failed: {e}"),
                            });
                        }
                    }
                    Some(WsCommand::Shutdown) | None => {
                        let _ = client.close().await;
                        break;
                    }
                }
            }
        }
    }
    tracing::debug!("ws loop ended");
}

fn parse_frame(frame: &ozzie_client::Frame) -> Option<ClientEvent> {
    match frame.event_kind() {
        Some(EventKind::AssistantStream) => {
            let content = frame
                .params
                .as_ref()?
                .get("content")?
                .as_str()
                .filter(|s| !s.is_empty())?;
            Some(ClientEvent::StreamDelta {
                content: content.to_string(),
            })
        }
        Some(EventKind::AssistantMessage) => {
            let p = frame.params.as_ref()?;
            if let Some(err) = p.get("error").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                return Some(ClientEvent::Error {
                    message: err.to_string(),
                });
            }
            let content = p
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ClientEvent::MessageComplete { content })
        }
        Some(EventKind::ToolCall) => {
            let p = frame.params.as_ref()?;
            let call_id = p
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = p
                .get("tool")
                .or_else(|| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let arguments = p
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}")
                .to_string();
            Some(ClientEvent::ToolCall {
                call_id,
                name,
                arguments,
            })
        }
        Some(EventKind::ToolResult) => {
            let p = frame.params.as_ref()?;
            let call_id = p
                .get("call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let result = p
                .get("result")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_error = p
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Some(ClientEvent::ToolResult {
                call_id,
                result,
                is_error,
            })
        }
        Some(EventKind::PromptRequest) => {
            let prompt: PromptRequestPayload = frame.parse_params().ok()?;
            Some(ClientEvent::PromptRequest {
                token: prompt.token,
                prompt_type: prompt.prompt_type,
                label: prompt.label,
            })
        }
        Some(EventKind::SkillStarted | EventKind::SkillStepStarted
            | EventKind::SkillStepCompleted | EventKind::SkillCompleted) => {
            let skill = frame
                .params
                .as_ref()
                .and_then(|p| p.get("skill"))
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            Some(ClientEvent::SkillEvent {
                event_type: frame.method.clone().unwrap_or_default(),
                skill,
            })
        }
        Some(EventKind::Error) => {
            let msg = frame
                .params
                .as_ref()
                .and_then(|p| p.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Some(ClientEvent::Error { message: msg })
        }
        _ => {
            tracing::debug!(event = frame.method.as_deref().unwrap_or("?"), "unhandled ws event");
            None
        }
    }
}
