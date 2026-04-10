use std::collections::HashSet;
use std::sync::Arc;

use ozzie_core::domain::{MemoryError, MemoryRetriever, PageStore, RetrievedMemory};
use tracing::warn;

/// Two-stage memory retriever: searches wiki pages first, then falls back to entries.
///
/// Pages are pre-synthesized thematic summaries with higher information density,
/// so they rank above individual memory entries. Entries already covered by a
/// returned page are deduplicated.
pub struct PageAwareRetriever {
    page_store: Arc<dyn PageStore>,
    entry_retriever: Arc<dyn MemoryRetriever>,
}

impl PageAwareRetriever {
    pub fn new(
        page_store: Arc<dyn PageStore>,
        entry_retriever: Arc<dyn MemoryRetriever>,
    ) -> Self {
        Self {
            page_store,
            entry_retriever,
        }
    }
}

#[async_trait::async_trait]
impl MemoryRetriever for PageAwareRetriever {
    async fn retrieve(
        &self,
        query: &str,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>, MemoryError> {
        let limit = if limit == 0 { 5 } else { limit };

        // Stage 1: Search wiki pages via FTS5
        let page_results = match self.page_store.search_text(query, limit).await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "page search failed, falling back to entries only");
                Vec::new()
            }
        };

        let mut results: Vec<RetrievedMemory> = Vec::new();
        let mut covered_entry_ids: HashSet<String> = HashSet::new();

        for pr in &page_results {
            match self.page_store.get(&pr.id).await {
                Ok((page, content)) => {
                    // Track which entries are covered by this page
                    for src_id in &page.source_ids {
                        covered_entry_ids.insert(src_id.clone());
                    }

                    results.push(RetrievedMemory {
                        id: pr.id.clone(),
                        title: pr.title.clone(),
                        memory_type: "page".to_string(),
                        content,
                        score: 2.0, // boost pages above entries
                        tags: pr.tags.clone(),
                    });
                }
                Err(e) => {
                    warn!(page_id = %pr.id, error = %e, "failed to load page content");
                }
            }
        }

        // Stage 2: Fill remaining slots with entries
        let remaining = limit.saturating_sub(results.len());
        if remaining > 0 {
            match self.entry_retriever.retrieve(query, tags, remaining * 2).await {
                Ok(entries) => {
                    for entry in entries {
                        if results.len() >= limit {
                            break;
                        }
                        // Skip entries already covered by a returned page
                        if covered_entry_ids.contains(&entry.id) {
                            continue;
                        }
                        results.push(entry);
                    }
                }
                Err(e) => {
                    warn!(error = %e, "entry retrieval failed");
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::{PageSearchResult, WikiPage};

    // Mock PageStore
    struct MockPageStore {
        pages: Vec<(WikiPage, String)>,
    }

    #[async_trait::async_trait]
    impl PageStore for MockPageStore {
        async fn upsert(&self, _page: &mut WikiPage, _content: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn get(&self, id: &str) -> Result<(WikiPage, String), MemoryError> {
            self.pages
                .iter()
                .find(|(p, _)| p.id == id)
                .cloned()
                .ok_or_else(|| MemoryError::Other(format!("not found: {id}")))
        }
        async fn get_by_slug(&self, _slug: &str) -> Result<(WikiPage, String), MemoryError> {
            unimplemented!()
        }
        async fn delete(&self, _id: &str) -> Result<(), MemoryError> {
            unimplemented!()
        }
        async fn list(&self) -> Result<Vec<WikiPage>, MemoryError> {
            Ok(self.pages.iter().map(|(p, _)| p.clone()).collect())
        }
        async fn search_text(
            &self,
            query: &str,
            _limit: usize,
        ) -> Result<Vec<PageSearchResult>, MemoryError> {
            Ok(self
                .pages
                .iter()
                .filter(|(p, c)| p.title.to_lowercase().contains(&query.to_lowercase()) || c.to_lowercase().contains(&query.to_lowercase()))
                .map(|(p, _)| PageSearchResult {
                    id: p.id.clone(),
                    title: p.title.clone(),
                    slug: p.slug.clone(),
                    tags: p.tags.clone(),
                })
                .collect())
        }
    }

    // Mock MemoryRetriever
    struct MockEntryRetriever {
        entries: Vec<RetrievedMemory>,
    }

    #[async_trait::async_trait]
    impl MemoryRetriever for MockEntryRetriever {
        async fn retrieve(
            &self,
            _query: &str,
            _tags: &[String],
            limit: usize,
        ) -> Result<Vec<RetrievedMemory>, MemoryError> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }
    }

    fn make_page(id: &str, title: &str, source_ids: &[&str]) -> (WikiPage, String) {
        (
            WikiPage {
                id: id.to_string(),
                title: title.to_string(),
                slug: id.replace("page_", ""),
                tags: vec!["test".to_string()],
                source_ids: source_ids.iter().map(|s| s.to_string()).collect(),
                created_at: Default::default(),
                updated_at: Default::default(),
                revision: 1,
            },
            format!("Content of {title}"),
        )
    }

    fn make_entry(id: &str, title: &str) -> RetrievedMemory {
        RetrievedMemory {
            id: id.to_string(),
            title: title.to_string(),
            memory_type: "fact".to_string(),
            content: format!("Content of {title}"),
            score: 1.0,
            tags: vec!["test".to_string()],
        }
    }

    #[tokio::test]
    async fn pages_rank_above_entries() {
        let page_store = Arc::new(MockPageStore {
            pages: vec![make_page("page_rust", "Rust Patterns", &["mem_1", "mem_2"])],
        });
        let entry_retriever = Arc::new(MockEntryRetriever {
            entries: vec![make_entry("mem_3", "Standalone Entry")],
        });

        let retriever = PageAwareRetriever::new(page_store, entry_retriever);
        let results = retriever.retrieve("rust", &[], 5).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].memory_type, "page");
        assert_eq!(results[0].title, "Rust Patterns");
        assert_eq!(results[1].title, "Standalone Entry");
    }

    #[tokio::test]
    async fn deduplicates_covered_entries() {
        let page_store = Arc::new(MockPageStore {
            pages: vec![make_page("page_rust", "Rust Patterns", &["mem_1", "mem_2"])],
        });
        let entry_retriever = Arc::new(MockEntryRetriever {
            entries: vec![
                make_entry("mem_1", "Covered Entry"),
                make_entry("mem_2", "Also Covered"),
                make_entry("mem_3", "Not Covered"),
            ],
        });

        let retriever = PageAwareRetriever::new(page_store, entry_retriever);
        let results = retriever.retrieve("rust", &[], 5).await.unwrap();

        // Page + mem_3 only (mem_1 and mem_2 are covered by the page)
        assert_eq!(results.len(), 2);
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"page_rust"));
        assert!(ids.contains(&"mem_3"));
        assert!(!ids.contains(&"mem_1"));
        assert!(!ids.contains(&"mem_2"));
    }

    #[tokio::test]
    async fn falls_back_to_entries_when_no_pages() {
        let page_store = Arc::new(MockPageStore { pages: vec![] });
        let entry_retriever = Arc::new(MockEntryRetriever {
            entries: vec![make_entry("mem_1", "Entry 1")],
        });

        let retriever = PageAwareRetriever::new(page_store, entry_retriever);
        let results = retriever.retrieve("test", &[], 5).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mem_1");
    }

    #[tokio::test]
    async fn respects_limit() {
        let page_store = Arc::new(MockPageStore {
            pages: vec![
                make_page("page_a", "Page A test", &[]),
                make_page("page_b", "Page B test", &[]),
            ],
        });
        let entry_retriever = Arc::new(MockEntryRetriever {
            entries: vec![make_entry("mem_1", "Entry 1")],
        });

        let retriever = PageAwareRetriever::new(page_store, entry_retriever);
        let results = retriever.retrieve("test", &[], 2).await.unwrap();

        assert_eq!(results.len(), 2);
    }
}
