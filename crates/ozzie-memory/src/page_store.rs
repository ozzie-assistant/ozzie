use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};
use tracing::{debug, info, warn};

use ozzie_core::domain::{MemoryError, PageSearchResult, PageStore, WikiPage};

use crate::page_frontmatter;

/// Wiki page store with markdown files as source of truth + SQLite FTS5 index.
///
/// Files live in `$OZZIE_PATH/memory/pages/{slug}.md`.
/// SQLite index in the same `memory.db` database (separate table).
pub struct MarkdownPageStore {
    dir: PathBuf,
    conn: Mutex<Connection>,
}

impl MarkdownPageStore {
    /// Opens or creates the page store.
    /// `db_path` should point to the shared `memory.db`.
    /// `pages_dir` is the directory for markdown files.
    pub fn new(pages_dir: &Path, db_path: &Path) -> Result<Self, MemoryError> {
        std::fs::create_dir_all(pages_dir)
            .map_err(|e| MemoryError::Other(format!("create pages dir: {e}")))?;

        let conn = Connection::open(db_path)
            .map_err(|e| MemoryError::Other(format!("open db: {e}")))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| MemoryError::Other(format!("pragmas: {e}")))?;

        let store = Self {
            dir: pages_dir.to_path_buf(),
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    /// Creates the page store from an existing shared connection.
    /// Avoids opening a second connection to the same database.
    pub fn with_connection(pages_dir: &Path, conn: Connection) -> Result<Self, MemoryError> {
        std::fs::create_dir_all(pages_dir)
            .map_err(|e| MemoryError::Other(format!("create pages dir: {e}")))?;

        let store = Self {
            dir: pages_dir.to_path_buf(),
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pages (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                slug        TEXT NOT NULL UNIQUE,
                tags        TEXT NOT NULL DEFAULT '[]',
                source_ids  TEXT NOT NULL DEFAULT '[]',
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                revision    INTEGER NOT NULL DEFAULT 1,
                content     TEXT NOT NULL DEFAULT ''
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS pages_fts USING fts5(
                title, content, tags,
                content=pages, content_rowid=rowid,
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS pages_ai AFTER INSERT ON pages BEGIN
                INSERT INTO pages_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS pages_ad AFTER DELETE ON pages BEGIN
                INSERT INTO pages_fts(pages_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS pages_au AFTER UPDATE ON pages BEGIN
                INSERT INTO pages_fts(pages_fts, rowid, title, content, tags)
                VALUES ('delete', old.rowid, old.title, old.content, old.tags);
                INSERT INTO pages_fts(rowid, title, content, tags)
                VALUES (new.rowid, new.title, new.content, new.tags);
            END;",
        )
        .map_err(|e| MemoryError::Other(format!("create pages tables: {e}")))?;
        Ok(())
    }

    /// Scans the pages directory for `.md` files and rebuilds the SQLite index.
    pub fn rebuild_index(&self) -> Result<usize, MemoryError> {
        let files = self.scan_markdown_files()?;
        let mut indexed = 0;
        let mut seen_slugs = std::collections::HashSet::new();

        for path in &files {
            match std::fs::read_to_string(path) {
                Ok(text) => match page_frontmatter::parse(&text) {
                    Ok((page, content)) => {
                        if page.id.is_empty() || page.slug.is_empty() {
                            warn!(path = %path.display(), "skipping page with empty id/slug");
                            continue;
                        }
                        seen_slugs.insert(page.slug.clone());
                        if let Err(e) = self.upsert_index(&page, &content) {
                            warn!(path = %path.display(), error = %e, "failed to index page file");
                        } else {
                            indexed += 1;
                        }
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse page file");
                    }
                },
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to read page file");
                }
            }
        }

        self.cleanup_orphans(&seen_slugs)?;
        info!(pages = indexed, files = files.len(), "page index rebuilt");
        Ok(indexed)
    }

    fn scan_markdown_files(&self) -> Result<Vec<PathBuf>, MemoryError> {
        let mut files = Vec::new();
        let dir = match std::fs::read_dir(&self.dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
            Err(e) => return Err(MemoryError::Other(format!("read pages dir: {e}"))),
        };

        for entry in dir {
            let entry = entry.map_err(|e| MemoryError::Other(e.to_string()))?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md")
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('_'))
            {
                files.push(path);
            }
        }
        Ok(files)
    }

    fn write_markdown_file(&self, page: &WikiPage, content: &str) -> Result<PathBuf, MemoryError> {
        let filename = page_frontmatter::filename_for(page);
        let path = self.dir.join(&filename);

        let markdown = page_frontmatter::serialize(page, content);

        let tmp = path.with_extension("md.tmp");
        std::fs::write(&tmp, &markdown)
            .map_err(|e| MemoryError::Other(format!("write page file: {e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| MemoryError::Other(format!("rename page file: {e}")))?;

        Ok(path)
    }

    fn delete_markdown_file(&self, slug: &str) -> Result<(), MemoryError> {
        let path = self.dir.join(format!("{slug}.md"));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| MemoryError::Other(format!("delete page file: {e}")))?;
        }
        Ok(())
    }

    fn upsert_index(&self, page: &WikiPage, content: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tags_json =
            serde_json::to_string(&page.tags).unwrap_or_else(|_| "[]".to_string());
        let source_ids_json =
            serde_json::to_string(&page.source_ids).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT OR REPLACE INTO pages
                (id, title, slug, tags, source_ids, created_at, updated_at, revision, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                page.id,
                page.title,
                page.slug,
                tags_json,
                source_ids_json,
                page.created_at.to_rfc3339(),
                page.updated_at.to_rfc3339(),
                page.revision,
                content,
            ],
        )
        .map_err(|e| MemoryError::Other(format!("upsert page index: {e}")))?;
        Ok(())
    }

    fn cleanup_orphans(
        &self,
        seen_slugs: &std::collections::HashSet<String>,
    ) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare("SELECT id, slug FROM pages")
            .map_err(|e| MemoryError::Other(e.to_string()))?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| MemoryError::Other(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        for (id, slug) in rows {
            if !seen_slugs.contains(&slug) {
                debug!(id = %id, slug = %slug, "removing orphan page index entry");
                let _ = conn.execute("DELETE FROM pages WHERE id = ?1", params![id]);
            }
        }
        Ok(())
    }

    fn generate_page_id(&self, slug: &str) -> String {
        format!("page_{slug}")
    }
}

#[async_trait::async_trait]
impl PageStore for MarkdownPageStore {
    async fn upsert(
        &self,
        page: &mut WikiPage,
        content: &str,
    ) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let now = Utc::now();

        // Check if slug already exists
        let existing: Option<(String, u32)> = conn
            .query_row(
                "SELECT id, revision FROM pages WHERE slug = ?1",
                params![page.slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((existing_id, existing_revision)) = existing {
            // Update existing page
            page.id = existing_id;
            page.revision = existing_revision + 1;
            page.updated_at = now;
        } else {
            // New page
            if page.id.is_empty() {
                page.id = self.generate_page_id(&page.slug);
            }
            page.created_at = now;
            page.updated_at = now;
            page.revision = 1;
        }

        let tags_json =
            serde_json::to_string(&page.tags).unwrap_or_else(|_| "[]".to_string());
        let source_ids_json =
            serde_json::to_string(&page.source_ids).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT OR REPLACE INTO pages
                (id, title, slug, tags, source_ids, created_at, updated_at, revision, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                page.id,
                page.title,
                page.slug,
                tags_json,
                source_ids_json,
                page.created_at.to_rfc3339(),
                page.updated_at.to_rfc3339(),
                page.revision,
                content,
            ],
        )
        .map_err(|e| MemoryError::Other(format!("upsert page: {e}")))?;
        drop(conn);

        // Write markdown file (SsoT)
        self.write_markdown_file(page, content)?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<(WikiPage, String), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT id, title, slug, tags, source_ids, created_at, updated_at, revision, content
                 FROM pages WHERE id = ?1",
            )
            .map_err(|e| MemoryError::Other(e.to_string()))?;

        stmt.query_row(params![id], |row| {
            let page = parse_page_row(row)?;
            let content: String = row.get(8)?;
            Ok((page, content))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                MemoryError::Other(format!("page {id:?} not found"))
            }
            other => MemoryError::Other(other.to_string()),
        })
    }

    async fn get_by_slug(&self, slug: &str) -> Result<(WikiPage, String), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT id, title, slug, tags, source_ids, created_at, updated_at, revision, content
                 FROM pages WHERE slug = ?1",
            )
            .map_err(|e| MemoryError::Other(e.to_string()))?;

        stmt.query_row(params![slug], |row| {
            let page = parse_page_row(row)?;
            let content: String = row.get(8)?;
            Ok((page, content))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                MemoryError::Other(format!("page with slug {slug:?} not found"))
            }
            other => MemoryError::Other(other.to_string()),
        })
    }

    async fn delete(&self, id: &str) -> Result<(), MemoryError> {
        // Get slug for file deletion
        let slug = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.query_row("SELECT slug FROM pages WHERE id = ?1", params![id], |row| {
                row.get::<_, String>(0)
            })
            .ok()
        };

        if let Some(slug) = &slug {
            self.delete_markdown_file(slug)?;
        }

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let rows = conn
            .execute("DELETE FROM pages WHERE id = ?1", params![id])
            .map_err(|e| MemoryError::Other(e.to_string()))?;

        if rows == 0 {
            return Err(MemoryError::Other(format!("page {id:?} not found")));
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<WikiPage>, MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT id, title, slug, tags, source_ids, created_at, updated_at, revision
                 FROM pages ORDER BY updated_at DESC",
            )
            .map_err(|e| MemoryError::Other(e.to_string()))?;

        let pages = stmt
            .query_map([], parse_page_row)
            .map_err(|e| MemoryError::Other(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(pages)
    }

    async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PageSearchResult>, MemoryError> {
        let limit = if limit == 0 { 10 } else { limit };
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.title, p.slug, p.tags
                 FROM pages_fts f
                 JOIN pages p ON p.rowid = f.rowid
                 WHERE pages_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| MemoryError::Other(e.to_string()))?;

        let results = stmt
            .query_map(params![query, limit], |row| {
                let tags_json: String = row.get(3)?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                Ok(PageSearchResult {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    slug: row.get(2)?,
                    tags,
                })
            })
            .map_err(|e| MemoryError::Other(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }
}

fn parse_page_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WikiPage> {
    let id: String = row.get(0)?;
    let title: String = row.get(1)?;
    let slug: String = row.get(2)?;
    let tags_json: String = row.get(3)?;
    let source_ids_json: String = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let updated_at_str: String = row.get(6)?;
    let revision: u32 = row.get(7)?;

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let source_ids: Vec<String> = serde_json::from_str(&source_ids_json).unwrap_or_default();
    let created_at = parse_datetime(&created_at_str);
    let updated_at = parse_datetime(&updated_at_str);

    Ok(WikiPage {
        id,
        title,
        slug,
        tags,
        source_ids,
        created_at,
        updated_at,
        revision,
    })
}

fn parse_datetime(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, MarkdownPageStore) {
        let dir = tempfile::tempdir().unwrap();
        let pages_dir = dir.path().join("pages");
        let db_path = dir.path().join("memory.db");
        let store = MarkdownPageStore::new(&pages_dir, &db_path).unwrap();
        (dir, store)
    }

    fn make_page(slug: &str, title: &str) -> WikiPage {
        WikiPage {
            id: String::new(),
            title: title.to_string(),
            slug: slug.to_string(),
            tags: vec!["test".to_string()],
            source_ids: vec!["mem_abc".to_string()],
            created_at: Default::default(),
            updated_at: Default::default(),
            revision: 0,
        }
    }

    #[tokio::test]
    async fn upsert_creates_new_page() {
        let (_dir, store) = setup();
        let mut page = make_page("rust-patterns", "Rust Patterns");
        store.upsert(&mut page, "Pattern content").await.unwrap();

        assert_eq!(page.id, "page_rust-patterns");
        assert_eq!(page.revision, 1);

        let (got, content) = store.get(&page.id).await.unwrap();
        assert_eq!(got.title, "Rust Patterns");
        assert_eq!(content, "Pattern content");
    }

    #[tokio::test]
    async fn upsert_updates_existing_page() {
        let (_dir, store) = setup();
        let mut page = make_page("deploy", "Deployment");
        store.upsert(&mut page, "v1").await.unwrap();
        assert_eq!(page.revision, 1);

        page.title = "Deployment Procedures".to_string();
        store.upsert(&mut page, "v2").await.unwrap();
        assert_eq!(page.revision, 2);

        let (got, content) = store.get(&page.id).await.unwrap();
        assert_eq!(got.title, "Deployment Procedures");
        assert_eq!(content, "v2");
    }

    #[tokio::test]
    async fn get_by_slug() {
        let (_dir, store) = setup();
        let mut page = make_page("arch", "Architecture");
        store.upsert(&mut page, "content").await.unwrap();

        let (got, _) = store.get_by_slug("arch").await.unwrap();
        assert_eq!(got.id, page.id);
    }

    #[tokio::test]
    async fn delete_removes_page_and_file() {
        let (_dir, store) = setup();
        let mut page = make_page("temp", "Temporary");
        store.upsert(&mut page, "bye").await.unwrap();

        store.delete(&page.id).await.unwrap();
        assert!(store.get(&page.id).await.is_err());

        let files = store.scan_markdown_files().unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn list_returns_all_pages() {
        let (_dir, store) = setup();
        let mut p1 = make_page("a", "Alpha");
        let mut p2 = make_page("b", "Beta");
        store.upsert(&mut p1, "a").await.unwrap();
        store.upsert(&mut p2, "b").await.unwrap();

        let pages = store.list().await.unwrap();
        assert_eq!(pages.len(), 2);
    }

    #[tokio::test]
    async fn fts_search() {
        let (_dir, store) = setup();
        let mut page = make_page("rust-errors", "Rust Error Handling");
        store
            .upsert(&mut page, "Use thiserror for typed errors")
            .await
            .unwrap();

        let results = store.search_text("thiserror", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug, "rust-errors");
    }

    #[tokio::test]
    async fn rebuild_index_from_files() {
        let (_dir, store) = setup();

        // Write a page file directly
        let md = "---\nid: page_manual\nslug: manual\ntags: [\"test\"]\nsource_ids: [\"mem_x\"]\nrevision: 1\ncreated: 2026-04-10T00:00:00Z\nupdated: 2026-04-10T00:00:00Z\n---\n\n# Manual Page\n\nWritten by hand.\n";
        std::fs::write(store.dir.join("manual.md"), md).unwrap();

        let count = store.rebuild_index().unwrap();
        assert_eq!(count, 1);

        let results = store.search_text("manual", 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn upsert_writes_markdown_file() {
        let (_dir, store) = setup();
        let mut page = make_page("file-check", "File Check");
        store.upsert(&mut page, "content here").await.unwrap();

        let files = store.scan_markdown_files().unwrap();
        assert_eq!(files.len(), 1);

        let text = std::fs::read_to_string(&files[0]).unwrap();
        assert!(text.contains("# File Check"));
        assert!(text.contains("content here"));
        assert!(text.contains("slug: file-check"));
    }
}
