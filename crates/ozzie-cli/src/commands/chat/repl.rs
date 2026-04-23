use std::io::{self, BufRead, Write as IoWrite};

use ozzie_client::{
    ClientError, EventKind, OpenConversationOpts, OzzieClient, PromptRequestPayload,
    PromptResponseParams,
};
use ozzie_utils::config::ozzie_path;

use super::input::{InputResult, InputState};
use super::markdown::MarkdownStream;
use super::spinner::SpinnerSet;
use super::ChatArgs;

pub async fn run(args: ChatArgs) -> anyhow::Result<()> {
    let token = OzzieClient::acquire_token_cli(&args.gateway, &ozzie_path()).await?;
    let mut client = OzzieClient::connect(&args.gateway, Some(&token)).await?;

    let conversation_id = client
        .open_session(OpenConversationOpts {
            conversation_id: args.session.as_deref(),
            working_dir: args.working_dir.as_deref(),
        })
        .await?;

    eprintln!("Connected — session {conversation_id}");
    eprintln!("Type /quit to exit.\n");

    if args.accept_all {
        client.accept_all_tools().await?;
    }

    let mut input_state = InputState::new();
    let prompt = "\x1b[1;34mozzie>\x1b[0m ";

    loop {
        let text = match input_state.read(prompt) {
            Ok(InputResult::Submit(text)) => text,
            Ok(InputResult::Cancel) => continue,
            Ok(InputResult::Quit) => break,
            Err(e) => {
                eprintln!("Input error: {e}");
                break;
            }
        };

        if text.is_empty() {
            continue;
        }

        // Slash commands
        if let Some(cmd) = text.strip_prefix('/') {
            match handle_slash(cmd) {
                SlashResult::Continue => continue,
                SlashResult::Quit => break,
            }
        }

        // Send message
        if let Err(e) = client.send_message(&text).await {
            eprintln!("Send error: {e}");
            break;
        }

        // Stream response
        if let Err(e) = drain_response(&mut client, args.accept_all).await {
            eprintln!("Error: {e}");
            break;
        }

        println!(); // blank line between turns
    }

    let _ = client.close().await;
    eprintln!("Bye.");
    Ok(())
}

/// Drains server events until the assistant finishes its response.
async fn drain_response(client: &mut OzzieClient, auto_accept: bool) -> anyhow::Result<()> {
    let mut md = MarkdownStream::new();
    let mut spinners = SpinnerSet::new();
    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(80));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            frame_result = client.read_frame() => {
                match frame_result {
                    Ok(frame) => {
                        if let Some(action) = process_frame(
                            &frame, client, &mut md, &mut spinners, auto_accept
                        ).await? {
                            return action;
                        }
                    }
                    Err(ClientError::Closed) => {
                        md.flush()?;
                        anyhow::bail!("Connection closed");
                    }
                    Err(e) => {
                        md.flush()?;
                        anyhow::bail!("Read error: {e}");
                    }
                }
            }
            _ = tick.tick(), if spinners.is_active() => {
                spinners.tick();
            }
        }
    }
}

/// Processes a single frame. Returns `Some(Ok(()))` to signal the turn is done.
async fn process_frame(
    frame: &ozzie_client::Frame,
    client: &mut OzzieClient,
    md: &mut MarkdownStream,
    spinners: &mut SpinnerSet,
    auto_accept: bool,
) -> anyhow::Result<Option<anyhow::Result<()>>> {
    if frame.is_notification() {
        match frame.event_kind() {
            Some(EventKind::AssistantStream) => {
                if let Some(ref params) = frame.params
                    && let Some(content) = params.get("content").and_then(|v| v.as_str())
                    && !content.is_empty()
                {
                    md.push(content)?;
                }
            }
            Some(EventKind::AssistantMessage) => {
                md.flush()?;
                println!();
                return Ok(Some(Ok(())));
            }
            Some(EventKind::ToolCall) => {
                md.flush()?;
                if let Some(ref params) = frame.params {
                    let name = params
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    spinners.start(name);
                }
            }
            Some(EventKind::ToolResult) => {
                if let Some(ref params) = frame.params {
                    let name = params
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let is_error = params
                        .get("is_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    spinners.finish(name, is_error);
                }
            }
            Some(EventKind::PromptRequest) => {
                md.flush()?;
                if let Ok(prompt) = frame.parse_params::<PromptRequestPayload>() {
                    handle_prompt(client, &prompt, auto_accept).await?;
                }
            }
            Some(
                EventKind::SkillStarted
                | EventKind::SkillStepStarted
                | EventKind::SkillStepCompleted
                | EventKind::SkillCompleted,
            ) => {
                if let Some(ref params) = frame.params {
                    let name = params
                        .get("skill")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let kind = frame.method.as_deref().unwrap_or("skill");
                    eprintln!("  \x1b[2m[{kind}: {name}]\x1b[0m");
                }
            }
            Some(EventKind::Error) => {
                md.flush()?;
                if let Some(ref params) = frame.params {
                    let msg = params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("\x1b[31mError: {msg}\x1b[0m");
                }
                return Ok(Some(Ok(())));
            }
            _ => {}
        }
    } else if frame.is_error() {
        md.flush()?;
        let err = frame.error_message().unwrap_or("unknown");
        eprintln!("\x1b[31mServer error: {err}\x1b[0m");
        return Ok(Some(Ok(())));
    }

    Ok(None)
}

enum SlashResult {
    Continue,
    Quit,
}

fn handle_slash(cmd: &str) -> SlashResult {
    match cmd.split_whitespace().next().unwrap_or("") {
        "quit" | "exit" | "q" => SlashResult::Quit,
        "clear" => {
            print!("\x1b[2J\x1b[H");
            let _ = io::stdout().flush();
            SlashResult::Continue
        }
        "help" => {
            eprintln!("  /quit     Exit the REPL");
            eprintln!("  /clear    Clear screen");
            eprintln!("  /help     Show this help");
            SlashResult::Continue
        }
        other => {
            eprintln!("Unknown command: /{other} — type /help");
            SlashResult::Continue
        }
    }
}

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

    eprintln!("\n\x1b[1;33m{}\x1b[0m", prompt.label);

    match prompt.prompt_type.as_str() {
        "confirm" | "select" => {
            eprint!("  [a]llow once / [A]lways / [d]eny > ");
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
            eprint!("  > ");
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
    fn slash_quit() {
        assert!(matches!(handle_slash("quit"), SlashResult::Quit));
        assert!(matches!(handle_slash("exit"), SlashResult::Quit));
        assert!(matches!(handle_slash("q"), SlashResult::Quit));
    }

    #[test]
    fn slash_unknown() {
        assert!(matches!(handle_slash("foobar"), SlashResult::Continue));
    }

    #[test]
    fn slash_help() {
        assert!(matches!(handle_slash("help"), SlashResult::Continue));
    }

    #[test]
    fn default_args() {
        let args = ChatArgs::default();
        assert_eq!(args.gateway, "http://127.0.0.1:18420");
        assert!(!args.accept_all);
        assert!(args.session.is_none());
    }
}
