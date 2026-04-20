use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use worm_memory::frontmatter;
use worm_memory::{
    MemoryEntry, MemoryEntryMeta, MemoryError, MemorySearchEntry, MemoryStore, Store,
};

use crate::SqliteStore;

/// Memory store with markdown files as the source of truth.
///
/// Writes go to markdown files first (SsoT), then to SQLite (denormalized index).
/// Reads go through SQLite FTS5 for fast full-text search.
/// The index is rebuilt from files at startup.
pub struct MarkdownStore {
    dir: PathBuf,
    index: SqliteStore,
}

impl MarkdownStore {
    /// Opens or creates the markdown memory store.
    /// Does NOT rebuild the index — call `rebuild_index()` explicitly after creation.
    pub fn new(dir: &Path) -> Result<Self, MemoryError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| MemoryError::Other(format!("create memory dir: {e}")))?;

        let index = SqliteStore::new(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            index,
        })
    }

    /// Scans the directory for `.md` files and rebuilds the SQLite index.
    /// Returns the number of entries indexed.
    pub fn rebuild_index(&self) -> Result<usize, MemoryError> {
        let files = self.scan_markdown_files()?;
        let mut indexed = 0;
        let mut seen_ids = HashSet::new();

        for path in &files {
            match std::fs::read_to_string(path) {
                Ok(text) => match frontmatter::parse(&text) {
                    Ok((entry, content)) => {
                        if entry.id.is_empty() {
                            warn!(path = %path.display(), "skipping file with empty id");
                            continue;
                        }
                        seen_ids.insert(entry.id.clone());
                        if let Err(e) = self.upsert_index(&entry, &content) {
                            warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to index memory file"
                            );
                        } else {
                            indexed += 1;
                        }
                    }
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "failed to parse memory file"
                        );
                    }
                },
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to read memory file");
                }
            }
        }

        // Clean orphan index entries (in SQLite but no file)
        self.cleanup_orphans(&seen_ids)?;

        info!(entries = indexed, files = files.len(), "memory index rebuilt");
        Ok(indexed)
    }

    /// Migrates existing SQLite-only entries to markdown files.
    /// Called once at startup if markdown files don't exist yet.
    pub async fn migrate_from_sqlite(&self) -> Result<usize, MemoryError> {
        let entries = self.index.list().await?;
        let mut migrated = 0;

        for entry in &entries {
            // Check if a markdown file already exists for this entry
            if self.find_file_by_id(&entry.id).is_some() {
                continue;
            }

            // Read content from SQLite
            let content = match self.index.get(&entry.id).await {
                Ok((_, c)) => c,
                Err(_) => continue,
            };

            // Write markdown file
            if let Err(e) = self.write_markdown_file(entry, &content) {
                warn!(id = %entry.id, error = %e, "failed to migrate memory to markdown");
            } else {
                migrated += 1;
            }
        }

        if migrated > 0 {
            info!(count = migrated, "migrated memories from SQLite to markdown");
        }
        Ok(migrated)
    }

    /// Returns the underlying SqliteStore (for FTS search access).
    pub fn sqlite(&self) -> &SqliteStore {
        &self.index
    }

    fn cleanup_orphans(&self, seen_ids: &HashSet<String>) -> Result<(), MemoryError> {
        // Direct SQLite query to list IDs, then delete orphans — avoids async
        let conn = self.index.connection().lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare("SELECT id FROM memories")
            .map_err(|e| MemoryError::Database(e.to_string()))?;
        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| MemoryError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for id in ids {
            if !seen_ids.contains(&id) {
                debug!(id = %id, "removing orphan index entry");
                if let Err(e) = conn.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id]) {
                    warn!(id = %id, error = %e, "failed to delete orphan index entry");
                }
            }
        }
        drop(conn);
        Ok(())
    }

    fn scan_markdown_files(&self) -> Result<Vec<PathBuf>, MemoryError> {
        let mut files = Vec::new();
        let dir = std::fs::read_dir(&self.dir)
            .map_err(|e| MemoryError::Other(format!("read memory dir: {e}")))?;

        for entry in dir {
            let entry = entry.map_err(|e| MemoryError::Other(e.to_string()))?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
        Ok(files)
    }

    fn write_markdown_file(
        &self,
        entry: &MemoryEntry,
        content: &str,
    ) -> Result<PathBuf, MemoryError> {
        let filename = frontmatter::filename_for(entry);
        let path = self.dir.join(&filename);

        // Remove old file if name changed (ID-based lookup)
        if let Some(old_path) = self.find_file_by_id(&entry.id)
            && old_path != path
            && let Err(e) = std::fs::remove_file(&old_path)
        {
            debug!(path = %old_path.display(), error = %e, "failed to remove old memory file");
        }

        let markdown = frontmatter::serialize(entry, content);

        // Atomic write: tmp + rename
        let tmp = path.with_extension("md.tmp");
        std::fs::write(&tmp, &markdown)
            .map_err(|e| MemoryError::Other(format!("write memory file: {e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| MemoryError::Other(format!("rename memory file: {e}")))?;

        Ok(path)
    }

    fn delete_markdown_file(&self, id: &str) -> Result<(), MemoryError> {
        if let Some(path) = self.find_file_by_id(id) {
            std::fs::remove_file(&path)
                .map_err(|e| MemoryError::Other(format!("delete memory file: {e}")))?;
        }
        Ok(())
    }

    /// Finds the markdown file for a given memory ID by scanning filenames.
    fn find_file_by_id(&self, id: &str) -> Option<PathBuf> {
        let short_id = if id.len() >= 4 {
            &id[id.len() - 4..]
        } else {
            id
        };

        let dir = std::fs::read_dir(&self.dir).ok()?;
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                let name = path.file_stem()?.to_string_lossy();
                if name.ends_with(short_id)
                    && let Ok(text) = std::fs::read_to_string(&path)
                    && let Ok((parsed, _)) = frontmatter::parse(&text)
                    && parsed.id == id
                {
                    return Some(path);
                }
            }
        }
        None
    }

    /// Upserts an entry into the SQLite index using direct SQL (no async).
    fn upsert_index(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError> {
        let conn = self.index.connection().lock().unwrap_or_else(|e| e.into_inner());
        let tags_json = serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".to_string());
        let indexed_at = entry.indexed_at.map(|t| t.to_rfc3339());
        let merged_into = entry.merged_into.as_deref();

        conn.execute(
            "INSERT OR REPLACE INTO memories
                (id, title, source, type, tags, created_at, updated_at, last_used_at,
                 confidence, importance, embedding_model, indexed_at, content, merged_into)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                entry.id,
                entry.title,
                entry.source,
                entry.memory_type.as_str(),
                tags_json,
                entry.created_at.to_rfc3339(),
                entry.updated_at.to_rfc3339(),
                entry.last_used_at.to_rfc3339(),
                entry.confidence,
                entry.importance.as_str(),
                entry.embedding_model,
                indexed_at,
                content,
                merged_into,
            ],
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Store for MarkdownStore {
    async fn create(&self, entry: &mut MemoryEntry, content: &str) -> Result<(), MemoryError> {
        // SQLite first (generates ID if empty)
        self.index.create(entry, content).await?;

        // Then write markdown file (SsoT)
        self.write_markdown_file(entry, content)?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<(MemoryEntry, String), MemoryError> {
        // Read from index (fast)
        self.index.get(id).await
    }

    async fn update(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError> {
        // Write markdown file first (SsoT)
        self.write_markdown_file(entry, content)?;

        // Then update index
        self.index.update(entry, content).await
    }

    async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        // Delete file first (SsoT)
        self.delete_markdown_file(id)?;

        // Then delete from index
        self.index.delete(id).await
    }

    async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.index.list().await
    }
}

#[async_trait::async_trait]
impl MemoryStore for MarkdownStore {
    async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemorySearchEntry>, MemoryError> {
        self.index.search_text(query, limit).await
    }

    async fn get_content(
        &self,
        id: &str,
    ) -> Result<String, MemoryError> {
        self.index.get_content(id).await
    }

    async fn list_entries(
        &self,
    ) -> Result<Vec<MemoryEntryMeta>, MemoryError> {
        self.index.list_entries().await
    }

    async fn get_entry(
        &self,
        id: &str,
    ) -> Result<(MemoryEntryMeta, String), MemoryError> {
        self.index.get_entry(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use worm_memory::{ImportanceLevel, MemoryType};

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
    async fn create_writes_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();

        let mut entry = make_entry("Test Memory");
        store.create(&mut entry, "Some content here").await.unwrap();

        // File should exist
        let files = store.scan_markdown_files().unwrap();
        assert_eq!(files.len(), 1);

        // File should contain frontmatter
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("---"));
        assert!(text.contains(&entry.id));
        assert!(text.contains("# Test Memory"));
        assert!(text.contains("Some content here"));
    }

    #[tokio::test]
    async fn rebuild_index_from_files() {
        let dir = tempfile::tempdir().unwrap();

        // Write a markdown file directly (simulating user edit)
        let md = "---\nid: mem_manual_test\ntype: fact\ntags: [manual]\nimportance: normal\nconfidence: 0.8\nsource: user\ncreated: 2026-03-23T00:00:00Z\nupdated: 2026-03-23T00:00:00Z\n---\n\n# Manual Entry\n\nThis was written by hand.\n";
        std::fs::write(dir.path().join("manual-entry_test.md"), md).unwrap();

        let store = MarkdownStore::new(dir.path()).unwrap();
        let count = store.rebuild_index().unwrap();
        assert_eq!(count, 1);

        // Should be searchable now
        let results = store.index.search_fts("manual", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mem_manual_test");
    }

    #[tokio::test]
    async fn delete_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();

        let mut entry = make_entry("To Delete");
        store.create(&mut entry, "bye").await.unwrap();

        let files_before = store.scan_markdown_files().unwrap();
        assert_eq!(files_before.len(), 1);

        store.delete(&entry.id).await.unwrap();

        let files_after = store.scan_markdown_files().unwrap();
        assert_eq!(files_after.len(), 0);
    }

    #[tokio::test]
    async fn update_overwrites_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();

        let mut entry = make_entry("Original Title");
        store.create(&mut entry, "v1").await.unwrap();

        entry.title = "Updated Title".to_string();
        entry.updated_at = Utc::now();
        store.update(&entry, "v2").await.unwrap();

        // New file should exist with updated content
        let files = store.scan_markdown_files().unwrap();
        assert_eq!(files.len(), 1);
        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("# Updated Title"));
        assert!(text.contains("v2"));
    }

    #[tokio::test]
    async fn migrate_from_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let store = MarkdownStore::new(dir.path()).unwrap();

        // Create via SQLite directly (simulating old data)
        let mut entry = make_entry("Legacy Memory");
        store.index.create(&mut entry, "old content").await.unwrap();

        // No markdown files yet
        let md_files: Vec<_> = store
            .scan_markdown_files()
            .unwrap()
            .into_iter()
            .filter(|p| !p.to_string_lossy().contains(".tmp"))
            .collect();
        assert_eq!(md_files.len(), 0);

        // Migrate
        let migrated = store.migrate_from_sqlite().await.unwrap();
        assert_eq!(migrated, 1);

        // Now markdown file exists
        let md_files = store.scan_markdown_files().unwrap();
        assert_eq!(md_files.len(), 1);
        let text = std::fs::read_to_string(&md_files[0]).unwrap();
        assert!(text.contains("# Legacy Memory"));
    }
}
