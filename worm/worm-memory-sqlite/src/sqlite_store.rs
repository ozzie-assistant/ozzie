use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use worm_memory::{MemoryEntry, MemoryEntryMeta, MemoryError, MemorySearchEntry, MemoryType, Store};

use crate::id::generate_id;

/// SQLite implementation with FTS5 full-text search.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Opens (or creates) the SQLite memory database.
    /// The database is stored in `{dir}/.cache/memory.db`.
    pub fn new(dir: &Path) -> Result<Self, MemoryError> {
        let cache_dir = dir.join(".cache");
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| MemoryError::Other(format!("create cache dir: {e}")))?;

        let db_path = cache_dir.join("memory.db");
        let conn = Connection::open(&db_path).map_err(|e| MemoryError::Database(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| MemoryError::Database(e.to_string()))?;

        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id              TEXT PRIMARY KEY,
                title           TEXT NOT NULL,
                source          TEXT NOT NULL DEFAULT '',
                type            TEXT NOT NULL CHECK(type IN ('preference','fact','procedure','context')),
                tags            TEXT NOT NULL DEFAULT '[]',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                last_used_at    TEXT NOT NULL,
                confidence      REAL NOT NULL DEFAULT 0.8,
                importance      TEXT NOT NULL DEFAULT 'normal'
                                CHECK(importance IN ('core','important','normal','ephemeral')),
                embedding_model TEXT NOT NULL DEFAULT '',
                indexed_at      TEXT,
                content         TEXT NOT NULL DEFAULT '',
                merged_into     TEXT REFERENCES memories(id) ON DELETE SET NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                title, content, tags,
                content=memories, content_rowid=rowid,
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
                INSERT INTO memories_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;",
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Performs a full-text search using FTS5.
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, MemoryError> {
        let limit = if limit == 0 { 10 } else { limit };
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.title, m.source, m.type, m.tags,
                    m.created_at, m.updated_at, m.last_used_at, m.confidence,
                    m.importance, m.embedding_model, m.indexed_at, m.merged_into
             FROM memories_fts f
             JOIN memories m ON m.rowid = f.rowid
             WHERE memories_fts MATCH ?1
             AND m.merged_into IS NULL
             ORDER BY rank
             LIMIT ?2",
            )
            .map_err(|e| MemoryError::Database(e.to_string()))?;

        let entries = stmt
            .query_map(params![query, limit], parse_entry_row)
            .map_err(|e| MemoryError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Updates LastUsedAt without changing content (lightweight reinforcement).
    pub fn touch(&self, id: &str, now: DateTime<Utc>) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE memories SET last_used_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), id],
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Updates only the confidence field.
    pub fn update_confidence(&self, id: &str, confidence: f64) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE memories SET confidence = ?1 WHERE id = ?2",
            params![confidence, id],
        )
        .map_err(|e| MemoryError::Database(e.to_string()))?;
        Ok(())
    }

    /// Returns the underlying connection (for vector store sharing).
    pub fn connection(&self) -> &Mutex<Connection> {
        &self.conn
    }
}

#[async_trait::async_trait]
impl Store for SqliteStore {
    async fn create(&self, entry: &mut MemoryEntry, content: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

        if entry.id.is_empty() {
            entry.id = generate_id("mem");
        }

        let now = Utc::now();
        if entry.created_at == DateTime::<Utc>::default() {
            entry.created_at = now;
        }
        if entry.updated_at == DateTime::<Utc>::default() {
            entry.updated_at = now;
        }
        if entry.last_used_at == DateTime::<Utc>::default() {
            entry.last_used_at = now;
        }
        if entry.confidence == 0.0 {
            entry.confidence = 0.8;
        }

        let tags_json = serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".to_string());
        let indexed_at = entry.indexed_at.map(|t| t.to_rfc3339());
        let merged_into = entry.merged_into.as_deref();

        conn.execute(
            "INSERT INTO memories
                (id, title, source, type, tags, created_at, updated_at, last_used_at,
                 confidence, importance, embedding_model, indexed_at, content, merged_into)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
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

    async fn get(&self, id: &str) -> Result<(MemoryEntry, String), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT id, title, source, type, tags, created_at, updated_at,
                    last_used_at, confidence, importance, embedding_model,
                    indexed_at, content, merged_into
             FROM memories WHERE id = ?1",
            )
            .map_err(|e| MemoryError::Database(e.to_string()))?;

        stmt.query_row(params![id], |row| {
            let entry = parse_entry_row(row)?;
            let content: String = row.get(12)?;
            Ok((entry, content))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                MemoryError::NotFound(format!("memory {id:?} not found"))
            }
            other => MemoryError::Database(other.to_string()),
        })
    }

    async fn update(&self, entry: &MemoryEntry, content: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tags_json = serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".to_string());
        let indexed_at = entry.indexed_at.map(|t| t.to_rfc3339());
        let merged_into = entry.merged_into.as_deref();

        let rows = conn
            .execute(
                "UPDATE memories SET
                title=?1, source=?2, type=?3, tags=?4, updated_at=?5,
                last_used_at=?6, confidence=?7, importance=?8,
                embedding_model=?9, indexed_at=?10, content=?11, merged_into=?12
             WHERE id=?13",
                params![
                    entry.title,
                    entry.source,
                    entry.memory_type.as_str(),
                    tags_json,
                    entry.updated_at.to_rfc3339(),
                    entry.last_used_at.to_rfc3339(),
                    entry.confidence,
                    entry.importance.as_str(),
                    entry.embedding_model,
                    indexed_at,
                    content,
                    merged_into,
                    entry.id,
                ],
            )
            .map_err(|e| MemoryError::Database(e.to_string()))?;

        if rows == 0 {
            return Err(MemoryError::NotFound(format!(
                "memory {:?} not found",
                entry.id
            )));
        }
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let rows = conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])
            .map_err(|e| MemoryError::Database(e.to_string()))?;
        if rows == 0 {
            return Err(MemoryError::NotFound(format!("memory {id:?} not found")));
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT id, title, source, type, tags, created_at, updated_at,
                    last_used_at, confidence, importance, embedding_model,
                    indexed_at, merged_into
             FROM memories WHERE merged_into IS NULL ORDER BY updated_at DESC",
            )
            .map_err(|e| MemoryError::Database(e.to_string()))?;

        let entries = stmt
            .query_map([], parse_entry_row)
            .map_err(|e| MemoryError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }
}

#[async_trait::async_trait]
impl worm_memory::MemoryStore for SqliteStore {
    async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemorySearchEntry>, MemoryError> {
        let entries = self.search_fts(query, limit)?;
        Ok(entries
            .into_iter()
            .map(|e| MemorySearchEntry {
                id: e.id,
                title: e.title,
                memory_type: e.memory_type.as_str().to_string(),
                tags: e.tags,
            })
            .collect())
    }

    async fn get_content(&self, id: &str) -> Result<String, MemoryError> {
        self.get(id).await.map(|(_, content)| content)
    }

    async fn list_entries(&self) -> Result<Vec<MemoryEntryMeta>, MemoryError> {
        let entries = self.list().await?;
        Ok(entries.into_iter().map(entry_to_meta).collect())
    }

    async fn get_entry(&self, id: &str) -> Result<(MemoryEntryMeta, String), MemoryError> {
        let (entry, content) = self.get(id).await?;
        Ok((entry_to_meta(entry), content))
    }
}

fn entry_to_meta(e: MemoryEntry) -> MemoryEntryMeta {
    MemoryEntryMeta {
        id: e.id,
        title: e.title,
        memory_type: e.memory_type.as_str().to_string(),
        tags: e.tags,
        source: e.source,
        importance: e.importance.as_str().to_string(),
        confidence: e.confidence,
        created_at: e.created_at,
        updated_at: e.updated_at,
    }
}

fn parse_entry_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let id: String = row.get(0)?;
    let title: String = row.get(1)?;
    let source: String = row.get(2)?;
    let type_str: String = row.get(3)?;
    let tags_json: String = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let updated_at_str: String = row.get(6)?;
    let last_used_at_str: String = row.get(7)?;
    let confidence: f64 = row.get(8)?;
    let importance_str: Option<String> = row.get(9)?;
    let embedding_model: String = row.get(10)?;
    let indexed_at_str: Option<String> = row.get(11)?;
    let merged_into: Option<String> = row.get(12)?;

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let memory_type = type_str.parse().unwrap_or(MemoryType::Fact);
    let importance = importance_str
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default();

    let created_at = parse_datetime(&created_at_str);
    let updated_at = parse_datetime(&updated_at_str);
    let last_used_at = parse_datetime(&last_used_at_str);
    let indexed_at = indexed_at_str.as_deref().map(parse_datetime);

    Ok(MemoryEntry {
        id,
        title,
        source,
        memory_type,
        tags,
        created_at,
        updated_at,
        last_used_at,
        confidence,
        importance,
        embedding_model,
        indexed_at,
        merged_into,
    })
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use worm_memory::ImportanceLevel;

    #[tokio::test]
    async fn create_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();

        let mut entry = MemoryEntry {
            id: String::new(),
            title: "Test memory".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec!["rust".to_string()],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.0,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };

        store.create(&mut entry, "Some content").await.unwrap();
        assert!(entry.id.starts_with("mem_"));
        assert!(entry.confidence > 0.0);

        let (got, content) = store.get(&entry.id).await.unwrap();
        assert_eq!(got.title, "Test memory");
        assert_eq!(content, "Some content");
    }

    #[tokio::test]
    async fn update_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();

        let mut entry = MemoryEntry {
            id: String::new(),
            title: "Original".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.0,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };

        store.create(&mut entry, "v1").await.unwrap();
        let id = entry.id.clone();

        entry.title = "Updated".to_string();
        entry.updated_at = Utc::now();
        store.update(&entry, "v2").await.unwrap();

        let (got, content) = store.get(&id).await.unwrap();
        assert_eq!(got.title, "Updated");
        assert_eq!(content, "v2");

        store.delete(&id).await.unwrap();
        assert!(store.get(&id).await.is_err());
    }

    #[tokio::test]
    async fn list_excludes_merged() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();

        let mut e1 = MemoryEntry {
            id: String::new(),
            title: "Active".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.0,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };
        store.create(&mut e1, "active").await.unwrap();

        let mut e2 = MemoryEntry {
            id: String::new(),
            title: "Merged".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.0,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: Some(e1.id.clone()),
        };
        store.create(&mut e2, "merged").await.unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Active");
    }

    #[tokio::test]
    async fn fts_search() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::new(dir.path()).unwrap();

        let mut entry = MemoryEntry {
            id: String::new(),
            title: "Rust programming".to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec!["programming".to_string()],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };
        store
            .create(&mut entry, "Rust is a systems programming language")
            .await
            .unwrap();

        let results = store.search_fts("programming", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].title, "Rust programming");
    }
}
