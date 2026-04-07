use clap::{Args, Subcommand};
use ozzie_utils::config::memory_path;
use ozzie_memory::{SqliteStore, Store};

use crate::output;

/// Memory management commands.
#[derive(Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    command: MemoryCommand,

    /// Output as JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum MemoryCommand {
    /// List all memories.
    List,
    /// Search memories by keyword.
    Search {
        /// Search query.
        query: String,
        /// Maximum results.
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Show a memory entry.
    Show {
        /// Memory ID.
        id: String,
    },
    /// Delete a memory entry.
    Forget {
        /// Memory ID.
        id: String,
    },
    /// Rebuild FTS index (re-insert all content triggers).
    Reindex,
    /// Run LLM-based consolidation (requires gateway).
    Consolidate {
        /// Gateway URL for LLM access.
        #[arg(long, default_value = "http://127.0.0.1:18420")]
        gateway: String,
    },
}

pub async fn run(args: MemoryArgs) -> anyhow::Result<()> {
    let memory_dir = memory_path();

    match args.command {
        MemoryCommand::List => list(&memory_dir, args.json).await,
        MemoryCommand::Search { query, limit } => search(&memory_dir, &query, limit, args.json).await,
        MemoryCommand::Show { id } => show(&memory_dir, &id, args.json).await,
        MemoryCommand::Forget { id } => forget(&memory_dir, &id).await,
        MemoryCommand::Reindex => reindex(&memory_dir).await,
        MemoryCommand::Consolidate { gateway } => consolidate(&gateway).await,
    }
}

async fn list(memory_dir: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let store = SqliteStore::new(memory_dir)?;
    let entries = store.list().await?;

    if json {
        return output::print_json(&entries);
    }

    if entries.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            vec![
                e.id.clone(),
                e.title.clone(),
                e.memory_type.as_str().to_string(),
                e.importance.as_str().to_string(),
                format!("{:.0}%", e.confidence * 100.0),
                e.updated_at.format("%Y-%m-%d %H:%M").to_string(),
            ]
        })
        .collect();

    output::print_table(&["ID", "TITLE", "TYPE", "IMPORTANCE", "CONF", "UPDATED"], rows);
    Ok(())
}

async fn search(
    memory_dir: &std::path::Path,
    query: &str,
    limit: usize,
    json: bool,
) -> anyhow::Result<()> {
    let store = SqliteStore::new(memory_dir)?;
    let entries = store.search_fts(query, limit)?;

    if json {
        return output::print_json(&entries);
    }

    if entries.is_empty() {
        println!("No results for \"{query}\".");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            vec![
                e.id.clone(),
                e.title.clone(),
                e.memory_type.as_str().to_string(),
                format!("{:.0}%", e.confidence * 100.0),
            ]
        })
        .collect();

    output::print_table(&["ID", "TITLE", "TYPE", "CONFIDENCE"], rows);
    Ok(())
}

async fn show(memory_dir: &std::path::Path, id: &str, json: bool) -> anyhow::Result<()> {
    let store = SqliteStore::new(memory_dir)?;
    let (entry, content) = store.get(id).await?;

    if json {
        let data = serde_json::json!({
            "entry": entry,
            "content": content,
        });
        return output::print_json(&data);
    }

    println!("ID: {}", entry.id);
    println!("Title: {}", entry.title);
    println!("Type: {}", entry.memory_type.as_str());
    println!("Source: {}", entry.source);
    println!("Tags: {}", entry.tags.join(", "));
    println!("Importance: {}", entry.importance.as_str());
    println!("Confidence: {:.0}%", entry.confidence * 100.0);
    println!("Created: {}", entry.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Updated: {}", entry.updated_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Last used: {}", entry.last_used_at.format("%Y-%m-%d %H:%M:%S"));
    if !entry.embedding_model.is_empty() {
        println!("Embedding model: {}", entry.embedding_model);
    }
    println!("\n--- Content ---");
    println!("{content}");

    Ok(())
}

async fn forget(memory_dir: &std::path::Path, id: &str) -> anyhow::Result<()> {
    let store = SqliteStore::new(memory_dir)?;
    store.delete(id).await?;
    println!("Memory {id} deleted.");
    Ok(())
}

async fn reindex(memory_dir: &std::path::Path) -> anyhow::Result<()> {
    let store = SqliteStore::new(memory_dir)?;
    let entries = store.list().await?;

    println!("Reindexing {} memories...", entries.len());

    // FTS index is maintained by triggers, so we just need to re-insert content
    // to force the FTS triggers to fire. In practice, this is a no-op since
    // SQLite FTS triggers run on INSERT/UPDATE/DELETE.
    for entry in &entries {
        let (_, content) = store.get(&entry.id).await?;
        let mut updated = entry.clone();
        updated.updated_at = chrono::Utc::now();
        store.update(&updated, &content).await?;
    }

    println!("Done. {} memories reindexed.", entries.len());
    Ok(())
}

async fn consolidate(gateway: &str) -> anyhow::Result<()> {
    // Consolidation requires LLM access through the gateway.
    // For now, we just signal that it needs a running gateway.
    let url = format!("{gateway}/api/health");
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            println!("Consolidation requires LLM access. Use the gateway API.");
            println!("POST {gateway}/api/memory/consolidate");
            Ok(())
        }
        _ => {
            anyhow::bail!("gateway not reachable at {gateway} — start it first");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_memory::{ImportanceLevel, MemoryEntry, MemoryType};

    fn make_entry(title: &str) -> MemoryEntry {
        MemoryEntry {
            id: String::new(),
            title: title.to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec!["test".to_string()],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.0,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        }
    }

    #[tokio::test]
    async fn list_empty() {
        let dir = tempfile::tempdir().unwrap();
        list(dir.path(), false).await.unwrap();
    }

    #[tokio::test]
    async fn list_with_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();
        let mut e = make_entry("Test fact");
        store.create(&mut e, "content").await.unwrap();

        list(dir.path(), false).await.unwrap();
        list(dir.path(), true).await.unwrap();
    }

    #[tokio::test]
    async fn search_memories() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();
        let mut e = make_entry("Rust programming");
        store.create(&mut e, "Rust is fast").await.unwrap();

        search(dir.path(), "programming", 10, false).await.unwrap();
    }

    #[tokio::test]
    async fn show_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();
        let mut e = make_entry("Show test");
        store.create(&mut e, "content here").await.unwrap();

        show(dir.path(), &e.id, false).await.unwrap();
        show(dir.path(), &e.id, true).await.unwrap();
    }

    #[tokio::test]
    async fn forget_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();
        let mut e = make_entry("To forget");
        store.create(&mut e, "content").await.unwrap();

        forget(dir.path(), &e.id).await.unwrap();
        assert!(store.get(&e.id).await.is_err());
    }

    #[tokio::test]
    async fn reindex_memories() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();
        let mut e = make_entry("Reindex test");
        store.create(&mut e, "reindex content").await.unwrap();

        reindex(dir.path()).await.unwrap();
    }
}
