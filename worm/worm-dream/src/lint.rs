use std::collections::HashSet;

use chrono::{Duration, Utc};
use tracing::warn;

use worm_memory::{MemorySchema, PageStore, Store};

/// A warning detected by the wiki linter.
#[derive(Debug, Clone)]
pub enum LintWarning {
    /// An entry older than 7 days that is not referenced by any page.
    OrphanEntry { entry_id: String, title: String },
    /// A page whose source entries are all deleted or merged.
    StalePage {
        page_id: String,
        slug: String,
        missing_sources: Vec<String>,
    },
    /// A page whose content exceeds the schema's max size.
    OversizedPage {
        page_id: String,
        slug: String,
        chars: usize,
        max: usize,
    },
}

/// Runs a lightweight lint pass over pages and entries.
pub async fn lint(
    page_store: &dyn PageStore,
    memory_store: &dyn Store,
    schema: Option<&MemorySchema>,
) -> Vec<LintWarning> {
    let mut warnings = Vec::new();

    let pages = match page_store.list().await {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "lint: failed to list pages");
            return warnings;
        }
    };

    let entries = match memory_store.list().await {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "lint: failed to list entries");
            return warnings;
        }
    };

    let entry_ids: HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();

    let covered: HashSet<String> = pages
        .iter()
        .flat_map(|p| p.source_ids.iter().cloned())
        .collect();

    // Orphan entries: >7 days old and not covered by any page
    let cutoff = Utc::now() - Duration::days(7);
    for entry in &entries {
        if entry.created_at < cutoff && !covered.contains(&entry.id) {
            warnings.push(LintWarning::OrphanEntry {
                entry_id: entry.id.clone(),
                title: entry.title.clone(),
            });
        }
    }

    // Stale pages: all source entries are gone
    for page in &pages {
        let missing: Vec<String> = page
            .source_ids
            .iter()
            .filter(|id| !entry_ids.contains(*id))
            .cloned()
            .collect();

        if !missing.is_empty() && missing.len() == page.source_ids.len() {
            warnings.push(LintWarning::StalePage {
                page_id: page.id.clone(),
                slug: page.slug.clone(),
                missing_sources: missing,
            });
        }
    }

    // Oversized pages
    if let Some(schema) = schema {
        for page in &pages {
            match page_store.get(&page.id).await {
                Ok((_, content)) if content.len() > schema.max_page_chars => {
                    warnings.push(LintWarning::OversizedPage {
                        page_id: page.id.clone(),
                        slug: page.slug.clone(),
                        chars: content.len(),
                        max: schema.max_page_chars,
                    });
                }
                _ => {}
            }
        }
    }

    for w in &warnings {
        match w {
            LintWarning::OrphanEntry { entry_id, title } => {
                warn!(id = %entry_id, title = %title, "lint: orphan entry (>7d, no page)");
            }
            LintWarning::StalePage { page_id, slug, .. } => {
                warn!(id = %page_id, slug = %slug, "lint: stale page (all sources gone)");
            }
            LintWarning::OversizedPage {
                page_id,
                slug,
                chars,
                max,
            } => {
                warn!(
                    id = %page_id, slug = %slug, chars = chars, max = max,
                    "lint: oversized page"
                );
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use worm_memory::{
        ImportanceLevel, MemoryEntry, MemoryError, MemoryType, PageSearchResult, WikiPage,
    };

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
        async fn search_text(
            &self,
            _: &str,
            _: usize,
        ) -> Result<Vec<PageSearchResult>, MemoryError> {
            unimplemented!()
        }
    }

    struct MockMemoryStore {
        entries: Vec<MemoryEntry>,
    }

    #[async_trait::async_trait]
    impl Store for MockMemoryStore {
        async fn create(&self, _: &mut MemoryEntry, _: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn get(&self, _: &str) -> Result<(MemoryEntry, String), MemoryError> {
            unimplemented!()
        }
        async fn update(&self, _: &MemoryEntry, _: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn delete(&self, _: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn list(&self) -> Result<Vec<MemoryEntry>, MemoryError> {
            Ok(self.entries.clone())
        }
    }

    fn make_entry(id: &str, title: &str, days_old: i64) -> MemoryEntry {
        let created = Utc::now() - Duration::days(days_old);
        MemoryEntry {
            id: id.to_string(),
            title: title.to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: vec![],
            created_at: created,
            updated_at: created,
            last_used_at: created,
            confidence: 0.8,
            importance: ImportanceLevel::Normal,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        }
    }

    fn make_page(slug: &str, source_ids: &[&str]) -> WikiPage {
        WikiPage {
            id: format!("page_{slug}"),
            title: slug.to_string(),
            slug: slug.to_string(),
            tags: vec![],
            source_ids: source_ids.iter().map(|s| s.to_string()).collect(),
            created_at: Default::default(),
            updated_at: Default::default(),
            revision: 1,
        }
    }

    #[tokio::test]
    async fn detects_orphan_entries() {
        let page_store = MockPageStore {
            pages: vec![make_page("rust", &["m1"])],
        };
        let memory_store = MockMemoryStore {
            entries: vec![
                make_entry("m1", "Covered", 10),
                make_entry("m2", "Old Orphan", 10),
                make_entry("m3", "New Entry", 1),
            ],
        };

        let warnings = lint(&page_store, &memory_store, None).await;
        let orphans: Vec<_> = warnings
            .iter()
            .filter(|w| matches!(w, LintWarning::OrphanEntry { .. }))
            .collect();
        assert_eq!(orphans.len(), 1);
    }

    #[tokio::test]
    async fn detects_stale_pages() {
        let page_store = MockPageStore {
            pages: vec![
                make_page("healthy", &["m1"]),
                make_page("stale", &["m_gone1", "m_gone2"]),
            ],
        };
        let memory_store = MockMemoryStore {
            entries: vec![make_entry("m1", "Exists", 1)],
        };

        let warnings = lint(&page_store, &memory_store, None).await;
        let stale: Vec<_> = warnings
            .iter()
            .filter(|w| matches!(w, LintWarning::StalePage { .. }))
            .collect();
        assert_eq!(stale.len(), 1);
    }

    #[tokio::test]
    async fn no_warnings_when_clean() {
        let page_store = MockPageStore {
            pages: vec![make_page("rust", &["m1"])],
        };
        let memory_store = MockMemoryStore {
            entries: vec![make_entry("m1", "Covered", 1)],
        };

        let warnings = lint(&page_store, &memory_store, None).await;
        assert!(warnings.is_empty());
    }
}
