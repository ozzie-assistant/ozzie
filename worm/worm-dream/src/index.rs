use chrono::Utc;

use worm_memory::{MemoryEntry, WikiPage};

/// Builds the `_index.md` content from pages and entries.
///
/// Returns the markdown string. Writing to disk is the caller's responsibility.
pub fn build_index_content(pages: &[WikiPage], entries: &[MemoryEntry]) -> String {
    let covered: std::collections::HashSet<String> = pages
        .iter()
        .flat_map(|p| p.source_ids.iter().cloned())
        .collect();

    let uncategorized: Vec<_> = entries
        .iter()
        .filter(|e| !covered.contains(&e.id))
        .collect();

    let mut content = String::new();
    content.push_str(&format!(
        "# Memory Wiki\n\nLast updated: {}\n\n",
        Utc::now().to_rfc3339()
    ));

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

    content.push_str(&format!(
        "---\n{} pages, {} entries ({} categorized, {} uncategorized)\n",
        pages.len(),
        entries.len(),
        entries.len() - uncategorized.len(),
        uncategorized.len(),
    ));

    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use worm_memory::{ImportanceLevel, MemoryType};

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

    #[test]
    fn builds_index_with_pages_and_uncategorized() {
        let pages = vec![make_page("rust", "Rust Patterns", &["m1", "m2"])];
        let entries = vec![
            make_entry("m1", "Entry 1"),
            make_entry("m2", "Entry 2"),
            make_entry("m3", "Orphan Entry"),
        ];

        let content = build_index_content(&pages, &entries);
        assert!(content.contains("# Memory Wiki"));
        assert!(content.contains("Rust Patterns"));
        assert!(content.contains("rust.md"));
        assert!(content.contains("Orphan Entry"));
        assert!(content.contains("1 pages"));
        assert!(content.contains("1 uncategorized"));
    }

    #[test]
    fn builds_empty_index() {
        let content = build_index_content(&[], &[]);
        assert!(content.contains("# Memory Wiki"));
        assert!(content.contains("0 pages"));
    }
}
