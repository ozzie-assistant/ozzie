use std::io::{self, BufRead, Write as IoWrite};

use clap::Args;
use ozzie_client::{ClientError, EventKind, OzzieClient, OpenSessionOpts, PromptRequestPayload, PromptResponseParams};
use ozzie_utils::config::ozzie_path;

/// Send a message and stream the response.
#[derive(Args)]
pub struct AskArgs {
    /// The message to send.
    message: String,

    /// Gateway URL.
    #[arg(long, default_value = "http://127.0.0.1:18420")]
    gateway: String,

    /// Conversation ID to resume.
    #[arg(short, long)]
    session: Option<String>,

    /// Accept all dangerous tools automatically.
    #[arg(short = 'y', long)]
    accept_all: bool,

    /// Timeout in seconds.
    #[arg(long, default_value = "300")]
    timeout: u64,

    /// Working directory for the session.
    #[arg(long)]
    working_dir: Option<String>,

    /// Skip TLS verification.
    #[arg(long)]
    insecure: bool,
}

pub async fn run(args: AskArgs) -> anyhow::Result<()> {
    let token = OzzieClient::acquire_token_cli(&args.gateway, &ozzie_path()).await?;
    let mut client = OzzieClient::connect(&args.gateway, Some(&token)).await?;

    // Open or resume session
    let session_id = client
        .open_session(OpenSessionOpts {
            session_id: args.session.as_deref(),
            working_dir: args.working_dir.as_deref(),
        })
        .await?;
    eprintln!("session: {session_id}");

    // Accept all tools if requested
    if args.accept_all {
        client.accept_all_tools().await?;
    }

    // Send message
    client.send_message(&args.message).await?;

    // Stream response
    let deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(args.timeout);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            eprintln!("\nTimeout after {}s", args.timeout);
            break;
        }

        match tokio::time::timeout(remaining, client.read_frame()).await {
            Ok(Ok(frame)) => {
                if frame.is_notification() {
                    match frame.event_kind() {
                        Some(EventKind::AssistantStream) => {
                            if let Some(ref params) = frame.params
                                && let Some(content) = params.get("content").and_then(|v| v.as_str())
                                && !content.is_empty()
                            {
                                print!("{content}");
                                io::stdout().flush()?;
                            }
                        }
                        Some(EventKind::AssistantMessage) => {
                            println!();
                            break;
                        }
                        Some(EventKind::ToolCall) => {
                            if let Some(ref params) = frame.params {
                                let name = params
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("?");
                                eprintln!("\n[tool: {name}]");
                            }
                        }
                        Some(EventKind::PromptRequest) => {
                            if let Ok(prompt) = frame.parse_params::<PromptRequestPayload>() {
                                handle_prompt(&mut client, &prompt, args.accept_all).await?;
                            }
                        }
                        Some(EventKind::SkillStarted | EventKind::SkillStepStarted
                            | EventKind::SkillStepCompleted | EventKind::SkillCompleted) => {
                            if let Some(ref params) = frame.params {
                                let name = params
                                    .get("skill")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("?");
                                let kind = frame.method.as_deref().unwrap_or("skill");
                                eprintln!("[{kind}: {name}]");
                            }
                        }
                        Some(EventKind::Error) => {
                            if let Some(ref params) = frame.params {
                                let msg = params
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown error");
                                eprintln!("\nError: {msg}");
                            }
                            break;
                        }
                        _ => {
                            tracing::debug!(event = frame.method.as_deref().unwrap_or("?"), "unhandled event");
                        }
                    }
                } else if frame.is_error() {
                    let err = frame.error_message().unwrap_or("unknown");
                    eprintln!("\nServer error: {err}");
                    break;
                }
            }
            Ok(Err(ClientError::Closed)) => {
                eprintln!("\nConnection closed.");
                break;
            }
            Ok(Err(e)) => {
                eprintln!("\nError: {e}");
                break;
            }
            Err(_) => {
                eprintln!("\nTimeout.");
                break;
            }
        }
    }

    let _ = client.close().await;
    Ok(())
}

/// Handles interactive prompts from the server.
async fn handle_prompt(
    client: &mut OzzieClient,
    prompt: &PromptRequestPayload,
    auto_accept: bool,
) -> anyhow::Result<()> {
    if auto_accept {
        client
            .respond_to_prompt(PromptResponseParams {
                token: prompt.token.clone(),
                value: Some("session".to_string()),
                text: None,
            })
            .await?;
        return Ok(());
    }

    eprintln!("\n{}", prompt.label);

    match prompt.prompt_type.as_str() {
        "confirm" | "select" => {
            eprint!("[a]llow once / [A]lways / [d]eny > ");
            io::stderr().flush()?;

            let mut input = String::new();
            io::stdin().lock().read_line(&mut input)?;
            let value = match input.trim() {
                "a" | "y" | "yes" => "once",
                "A" | "always" => "session",
                _ => "deny",
            };
            client
                .respond_to_prompt(PromptResponseParams {
                    token: prompt.token.clone(),
                    value: Some(value.to_string()),
                    text: None,
                })
                .await?;
        }
        "text" => {
            eprint!("> ");
            io::stderr().flush()?;

            let mut input = String::new();
            io::stdin().lock().read_line(&mut input)?;
            client
                .respond_to_prompt(PromptResponseParams {
                    token: prompt.token.clone(),
                    value: None,
                    text: Some(input.trim().to_string()),
                })
                .await?;
        }
        _ => {
            client
                .respond_to_prompt(PromptResponseParams {
                    token: prompt.token.clone(),
                    value: Some("deny".to_string()),
                    text: None,
                })
                .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_defaults() {
        let args = AskArgs {
            message: "hello".to_string(),
            gateway: "http://127.0.0.1:18420".to_string(),
            session: None,
            accept_all: false,
            timeout: 300,
            working_dir: None,
            insecure: false,
        };
        assert_eq!(args.timeout, 300);
        assert!(!args.accept_all);
    }

    #[test]
    fn prompt_payload_parse() {
        let payload = serde_json::json!({
            "token": "tok_123",
            "prompt_type": "confirm",
            "message": "Allow shell command?",
        });
        let token = payload["token"].as_str().unwrap();
        let pt = payload["prompt_type"].as_str().unwrap();
        assert_eq!(token, "tok_123");
        assert_eq!(pt, "confirm");
    }
}
