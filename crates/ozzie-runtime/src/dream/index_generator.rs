use std::path::Path;

use chrono::Utc;
use tracing::info;

use ozzie_core::domain::{PageStore, WikiPage};
use ozzie_memory::Store;

/// Generates `_index.md` in the pages directory after each synthesis pass.
///
/// The index is a human-readable table of contents — not used by the retriever,
/// but useful for browsing and debugging the wiki.
pub async fn generate_index(
    pages_dir: &Path,
    page_store: &dyn PageStore,
    memory_store: &dyn Store,
) -> anyhow::Result<()> {
    let pages = page_store
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let entries = memory_store
        .list()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Collect all entry IDs referenced by pages
    let covered: std::collections::HashSet<String> = pages
        .iter()
        .flat_map(|p| p.source_ids.iter().cloned())
        .collect();

    // Find uncategorized entries (not covered by any page)
    let uncategorized: Vec<_> = entries
        .iter()
        .filter(|e| !covered.contains(&e.id))
        .collect();

    let mut content = String::new();
    content.push_str(&format!(
        "# Memory Wiki\n\nLast updated: {}\n\n",
        Utc::now().to_rfc3339()
    ));

    // Pages section
    if !pages.is_empty() {
        content.push_str("## Pages\n\n");
        let mut sorted_pages: Vec<&WikiPage> = pages.iter().collect();
        sorted_pages.sort_by(|a, b| a.title.cmp(&b.title));

        for page in &sorted_pages {
            content.push_str(&format!(
                "- [{}]({}.md) — {} sources, rev {}\n",
                page.title,
                page.slug,
                page.source_ids.len(),
                page.revision,
            ));
        }
        content.push('\n');
    }

    // Uncategorized entries section
    if !uncategorized.is_empty() {
        content.push_str("## Uncategorized Entries\n\n");
        for entry in &uncategorized {
            content.push_str(&format!(
                "- {} ({}, {})\n",
                entry.title,
                entry.memory_type.as_str(),
                entry.created_at.format("%Y-%m-%d"),
            ));
        }
        content.push('\n');
    }

    // Stats
    content.push_str(&format!(
        "---\n{} pages, {} entries ({} categorized, {} uncategorized)\n",
        pages.len(),
        entries.len(),
        entries.len() - uncategorized.len(),
        uncategorized.len(),
    ));

    // Write atomically
    let index_path = pages_dir.join("_index.md");
    let tmp = index_path.with_extension("md.tmp");
    std::fs::write(&tmp, &content)
        .map_err(|e| anyhow::anyhow!("write index: {e}"))?;
    std::fs::rename(&tmp, &index_path)
        .map_err(|e| anyhow::anyhow!("rename index: {e}"))?;

    info!(
        pages = pages.len(),
        uncategorized = uncategorized.len(),
        "wiki index generated"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::{MemoryError, PageSearchResult};
    use ozzie_memory::{ImportanceLevel, MemoryEntry, MemoryType};

    struct MockPageStore {
        pages: Vec<WikiPage>,
    }

    #[async_trait::async_trait]
    impl PageStore for MockPageStore {
        async fn upsert(&self, _: &mut WikiPage, _: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn get(&self, _: &str) -> Result<(WikiPage, String), MemoryError> {
            unimplemented!()
        }
        async fn get_by_slug(&self, _: &str) -> Result<(WikiPage, String), MemoryError> {
            unimplemented!()
        }
        async fn delete(&self, _: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn list(&self) -> Result<Vec<WikiPage>, MemoryError> {
            Ok(self.pages.clone())
        }
        async fn search_text(&self, _: &str, _: usize) -> Result<Vec<PageSearchResult>, MemoryError> {
            unimplemented!()
        }
    }

    struct MockMemoryStore {
        entries: Vec<(MemoryEntry, String)>,
    }

    #[async_trait::async_trait]
    impl Store for MockMemoryStore {
        async fn create(&self, _: &mut MemoryEntry, _: &str) -> Result<(), ozzie_memory::MemoryError> {
            unimplemented!()
        }
        async fn get(&self, _: &str) -> Result<(MemoryEntry, String), ozzie_memory::MemoryError> {
            unimplemented!()
        }
        async fn update(&self, _: &MemoryEntry, _: &str) -> Result<(), ozzie_memory::MemoryError> {
            unimplemented!()
        }
        async fn delete(&self, _: &str) -> Result<(), ozzie_memory::MemoryError> {
            unimplemented!()
        }
        async fn list(&self) -> Result<Vec<MemoryEntry>, ozzie_memory::MemoryError> {
            Ok(self.entries.iter().map(|(e, _)| e.clone()).collect())
        }
    }

    fn make_entry(id: &str, title: &str) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            title: title.to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
            last_used_at: Default::default(),
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        }
    }

    fn make_page(slug: &str, title: &str, source_ids: &[&str]) -> WikiPage {
        WikiPage {
            id: format!("page_{slug}"),
            title: title.to_string(),
            slug: slug.to_string(),
            tags: vec![],
            source_ids: source_ids.iter().map(|s| s.to_string()).collect(),
            created_at: Default::default(),
            updated_at: Default::default(),
            revision: 1,
        }
    }

    #[tokio::test]
    async fn generates_index_with_pages_and_uncategorized() {
        let dir = tempfile::tempdir().unwrap();
        let pages_dir = dir.path().join("pages");
        std::fs::create_dir_all(&pages_dir).unwrap();

        let page_store = MockPageStore {
            pages: vec![make_page("rust", "Rust Patterns", &["m1", "m2"])],
        };
        let memory_store = MockMemoryStore {
            entries: vec![
                (make_entry("m1", "Entry 1"), String::new()),
                (make_entry("m2", "Entry 2"), String::new()),
                (make_entry("m3", "Orphan Entry"), String::new()),
            ],
        };

        generate_index(&pages_dir, &page_store, &memory_store)
            .await
            .unwrap();

        let content = std::fs::read_to_string(pages_dir.join("_index.md")).unwrap();
        assert!(content.contains("# Memory Wiki"));
        assert!(content.contains("Rust Patterns"));
        assert!(content.contains("rust.md"));
        assert!(content.contains("Orphan Entry"));
        assert!(content.contains("1 pages"));
        assert!(content.contains("1 uncategorized"));
    }

    #[tokio::test]
    async fn generates_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let pages_dir = dir.path().join("pages");
        std::fs::create_dir_all(&pages_dir).unwrap();

        let page_store = MockPageStore { pages: vec![] };
        let memory_store = MockMemoryStore { entries: vec![] };

        generate_index(&pages_dir, &page_store, &memory_store)
            .await
            .unwrap();

        let content = std::fs::read_to_string(pages_dir.join("_index.md")).unwrap();
        assert!(content.contains("# Memory Wiki"));
        assert!(content.contains("0 pages"));
    }
}
