use clap::{Args, Subcommand};
use ozzie_utils::config::conversations_path;
use ozzie_utils::names;
use ozzie_runtime::FileConversationStore;
use ozzie_runtime::conversation::ConversationStore;

use crate::output;

/// Conversation management commands.
#[derive(Args)]
pub struct ConversationsArgs {
    #[command(subcommand)]
    command: ConversationsCommand,

    /// Output as JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum ConversationsCommand {
    /// List all sessions.
    List,
    /// Show session details.
    Show {
        /// Conversation ID.
        id: String,
    },
}

pub async fn run(args: ConversationsArgs) -> anyhow::Result<()> {
    let conversations_dir = conversations_path();
    let store = FileConversationStore::new(&conversations_dir)?;

    match args.command {
        ConversationsCommand::List => list(&store, args.json).await,
        ConversationsCommand::Show { id } => show(&store, &id, args.json).await,
    }
}

async fn list(store: &FileConversationStore, json: bool) -> anyhow::Result<()> {
    let sessions = store.list().await?;

    if json {
        return output::print_json(&sessions);
    }

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    let mut rows = Vec::new();
    for s in &sessions {
        let tokens = if s.token_usage.is_zero() {
            "-".to_string()
        } else {
            format!("{}↓ {}↑", s.token_usage.input, s.token_usage.output)
        };
        rows.push(vec![
            s.id.clone(),
            names::display_name_pretty(&s.id),
            s.status.to_string(),
            s.model.clone().unwrap_or_default(),
            format!("{}", s.message_count),
            tokens,
            s.updated_at.format("%Y-%m-%d %H:%M").to_string(),
        ]);
    }

    output::print_table(&["ID", "NAME", "STATUS", "MODEL", "MESSAGES", "TOKENS", "UPDATED"], rows);
    Ok(())
}

async fn show(store: &FileConversationStore, id: &str, json: bool) -> anyhow::Result<()> {
    let session = store
        .get(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))?;

    let messages = store.load_messages(id).await?;

    if json {
        let data = serde_json::json!({
            "session": session,
            "messages": messages,
        });
        return output::print_json(&data);
    }

    println!("Conversation: {} ({})", session.id, names::display_name_pretty(&session.id));
    println!("Status:  {}", session.status);
    println!("Created: {}", session.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Updated: {}", session.updated_at.format("%Y-%m-%d %H:%M:%S"));
    if let Some(ref model) = session.model {
        println!("Model:   {model}");
    }
    if let Some(ref dir) = session.root_dir {
        println!("Root dir: {dir}");
    }
    if let Some(ref summary) = session.summary {
        println!("Summary: {summary}");
    }
    println!("Messages: {}", session.message_count);
    if !session.token_usage.is_zero() {
        println!(
            "Tokens:   {} input, {} output",
            session.token_usage.input, session.token_usage.output
        );
    }
    if !session.metadata.is_empty() {
        println!("Metadata:");
        for (k, v) in &session.metadata {
            println!("  {k}: {v}");
        }
    }
    println!("\n--- Messages ({}) ---", messages.len());
    for msg in &messages {
        let ts = msg
            .ts
            .map(|t| t.format("%H:%M:%S").to_string())
            .unwrap_or_default();
        println!("[{ts}] {}: {}", msg.role, msg.content);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::Message;
    use ozzie_runtime::Conversation;

    #[tokio::test]
    async fn list_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();
        list(&store, false).await.unwrap();
    }

    #[tokio::test]
    async fn list_with_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        let session = Conversation::new("sess_test_one");
        store.create(&session).await.unwrap();
        store
            .append_message("sess_test_one", Message::user("hello"))
            .await
            .unwrap();

        list(&store, false).await.unwrap();
        list(&store, true).await.unwrap();
    }

    #[tokio::test]
    async fn show_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        let mut session = Conversation::new("sess_show_test");
        session.root_dir = Some("/tmp".to_string());
        session.summary = Some("test summary".to_string());
        store.create(&session).await.unwrap();
        store
            .append_message("sess_show_test", Message::user("hi"))
            .await
            .unwrap();
        store
            .append_message("sess_show_test", Message::assistant("hello"))
            .await
            .unwrap();

        show(&store, "sess_show_test", false).await.unwrap();
        show(&store, "sess_show_test", true).await.unwrap();
    }

    #[tokio::test]
    async fn show_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();
        let result = show(&store, "nonexistent", false).await;
        assert!(result.is_err());
    }
}
