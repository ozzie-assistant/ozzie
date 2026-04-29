use ozzie_client::{ClientError, EventKind, OzzieClient, PromptRequestPayload};
use ozzie_types::{PromptOption, events::ToolResultEvent};
use tokio::sync::mpsc;

/// TUI-friendly projection of gateway EventKind frames.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    #[allow(dead_code)]
    ConversationReady(String),
    AssistantDelta(String),
    AssistantDone,
    ToolStart { call_id: String, tool: String, args: String },
    #[allow(dead_code)]
    ToolProgress { call_id: String, message: String },
    ToolResult { call_id: String, #[allow(dead_code)] tool: String, result: String, is_error: bool },
    ApprovalRequest { token: String, label: String, options: Vec<PromptOption> },
    AgentDone,
    AgentCancelled(#[allow(dead_code)] String),
    Error(String),
    ConnectionClosed,
}

/// Messages sortants que l'App peut envoyer au gateway.
#[derive(Debug)]
pub enum OutboundMsg {
    SendMessage(String),
    /// Réponse à un `PromptRequest` (approbation outil, etc.).
    PromptResponse { token: String, value: String },
}

/// Lance deux tâches tokio :
/// - une pour lire les frames du gateway et les traduire en `ServerEvent`
/// - une pour envoyer les messages sortants vers le gateway
///
/// Retourne un `Sender` pour les messages sortants.
pub fn spawn_bridge(
    mut client: OzzieClient,
    tx: mpsc::UnboundedSender<ServerEvent>,
) -> mpsc::UnboundedSender<OutboundMsg> {
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<OutboundMsg>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                frame_result = client.read_frame() => {
                    match frame_result {
                        Ok(frame) => {
                            if let Some(ev) = translate_frame(&frame) {
                                let done = matches!(ev, ServerEvent::ConnectionClosed);
                                if tx.send(ev).is_err() {
                                    break;
                                }
                                if done {
                                    break;
                                }
                            }
                        }
                        Err(ClientError::Closed) => {
                            let _ = tx.send(ServerEvent::ConnectionClosed);
                            break;
                        }
                        Err(e) => {
                            let _ = tx.send(ServerEvent::Error(e.to_string()));
                            break;
                        }
                    }
                }
                Some(msg) = out_rx.recv() => {
                    match msg {
                        OutboundMsg::SendMessage(text) => {
                            if let Err(e) = client.send_message(&text).await {
                                let _ = tx.send(ServerEvent::Error(e.to_string()));
                            }
                        }
                        OutboundMsg::PromptResponse { token, value } => {
                            use ozzie_client::PromptResponseParams;
                            let params = PromptResponseParams {
                                token,
                                value: Some(value),
                                text: None,
                            };
                            if let Err(e) = client.respond_to_prompt(params).await {
                                let _ = tx.send(ServerEvent::Error(e.to_string()));
                            }
                        }
                    }
                }
            }
        }
    });

    out_tx
}

fn translate_frame(frame: &ozzie_client::Frame) -> Option<ServerEvent> {
    if frame.is_notification() {
        match frame.event_kind() {
            Some(EventKind::AssistantStream) => {
                let content = frame
                    .params
                    .as_ref()
                    .and_then(|p| p.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                if content.is_empty() {
                    None
                } else {
                    Some(ServerEvent::AssistantDelta(content))
                }
            }
            Some(EventKind::AssistantMessage) => Some(ServerEvent::AssistantDone),
            Some(EventKind::ToolCall) => {
                let params = frame.params.as_ref()?;
                Some(ServerEvent::ToolStart {
                    call_id: params
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    tool: params
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_owned(),
                    args: params
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}")
                        .to_owned(),
                })
            }
            Some(EventKind::ToolProgress) => {
                let params = frame.params.as_ref()?;
                Some(ServerEvent::ToolProgress {
                    call_id: params
                        .get("call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    message: params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned(),
                })
            }
            Some(EventKind::ToolResult) => {
                let ev: ToolResultEvent =
                    frame.parse_params().ok()?;
                Some(ServerEvent::ToolResult {
                    call_id: ev.call_id,
                    tool: ev.tool,
                    result: ev.result,
                    is_error: ev.is_error,
                })
            }
            Some(EventKind::PromptRequest) => {
                let prompt: PromptRequestPayload = frame.parse_params().ok()?;
                Some(ServerEvent::ApprovalRequest {
                    token: prompt.token,
                    label: prompt.label,
                    options: prompt.options,
                })
            }
            Some(EventKind::AgentYielded) => Some(ServerEvent::AgentDone),
            Some(EventKind::AgentCancelled) => {
                let reason = frame
                    .params
                    .as_ref()
                    .and_then(|p| p.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("cancelled")
                    .to_owned();
                Some(ServerEvent::AgentCancelled(reason))
            }
            Some(EventKind::Error) => {
                let msg = frame
                    .params
                    .as_ref()
                    .and_then(|p| p.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_owned();
                Some(ServerEvent::Error(msg))
            }
            _ => None,
        }
    } else if frame.is_error() {
        Some(ServerEvent::Error(
            frame.error_message().unwrap_or("server error").to_owned(),
        ))
    } else {
        None
    }
}
