use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::{debug, info, warn};

use ozzie_core::domain::{PageStore, WikiPage};
use ozzie_llm::{ChatMessage, ChatRole, Provider};
use ozzie_memory::Store;

const SYNTHESIS_PROMPT: &str = r#"You are creating a wiki-style knowledge page from related memory entries.
Synthesize a coherent, well-structured markdown page that captures all the information.
Use markdown headers (##) for structure. Be comprehensive but not redundant.
Write in the same language as the source entries.

Respond with JSON only, no markdown fences:
{"title": "Page Title", "slug": "url-safe-slug", "content": "markdown content...", "tags": ["tag1", "tag2"]}"#;

const UPDATE_PROMPT: &str = r#"You are updating an existing wiki page with new information from memory entries.
Merge the new entries into the existing page, preserving structure and removing redundancy.
Write in the same language as the source entries.

Respond with JSON only, no markdown fences:
{"title": "Page Title", "slug": "url-safe-slug", "content": "updated markdown content...", "tags": ["tag1", "tag2"]}"#;

/// Minimum entries sharing 2+ tags to form a cluster.
const MIN_CLUSTER_SIZE: usize = 3;
/// Minimum shared tags for two entries to belong to the same cluster.
const MIN_SHARED_TAGS: usize = 2;

/// Statistics from a synthesis pass.
#[derive(Debug, Default)]
pub struct SynthesisStats {
    pub pages_created: usize,
    pub pages_updated: usize,
    pub clusters_skipped: usize,
}

/// Synthesizes wiki pages from memory entry clusters.
///
/// Runs after the dream classification step. Groups entries by shared tags,
/// then creates or updates wiki pages for each cluster via LLM.
pub struct Synthesizer {
    memory_store: Arc<dyn Store>,
    page_store: Arc<dyn PageStore>,
    provider: Arc<dyn Provider>,
}

impl Synthesizer {
    pub fn new(
        memory_store: Arc<dyn Store>,
        page_store: Arc<dyn PageStore>,
        provider: Arc<dyn Provider>,
    ) -> Self {
        Self {
            memory_store,
            page_store,
            provider,
        }
    }

    /// Groups all active entries by tag affinity and creates/updates pages.
    pub async fn synthesize(&self) -> anyhow::Result<SynthesisStats> {
        let mut stats = SynthesisStats::default();

        // Load all active entries
        let entries = self.memory_store.list().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        if entries.len() < MIN_CLUSTER_SIZE {
            debug!(entries = entries.len(), "too few entries for synthesis");
            return Ok(stats);
        }

        // Load existing pages to check coverage
        let existing_pages = self
            .page_store
            .list()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let existing_source_map = build_source_map(&existing_pages);

        // Build clusters from tag affinity
        let clusters = cluster_by_tags(&entries);

        for cluster in &clusters {
            if cluster.entry_ids.len() < MIN_CLUSTER_SIZE {
                continue;
            }

            // Check if an existing page already covers this cluster
            let (covered, uncovered) =
                partition_coverage(&cluster.entry_ids, &existing_source_map);

            if uncovered.is_empty() && !covered.is_empty() {
                // All entries already covered — skip
                stats.clusters_skipped += 1;
                continue;
            }

            if !covered.is_empty() && !uncovered.is_empty() {
                // Some entries are new — update the existing page
                let page_id = find_best_page_for_cluster(&cluster.entry_ids, &existing_source_map);
                if let Some(page_id) = page_id {
                    match self.update_page(&page_id, &uncovered).await {
                        Ok(()) => {
                            stats.pages_updated += 1;
                            debug!(page_id = %page_id, new_entries = uncovered.len(), "page updated");
                        }
                        Err(e) => {
                            warn!(page_id = %page_id, error = %e, "failed to update page");
                        }
                    }
                    continue;
                }
            }

            // New cluster — synthesize a new page
            match self.create_page(&cluster.entry_ids, &cluster.shared_tags).await {
                Ok(slug) => {
                    stats.pages_created += 1;
                    debug!(slug = %slug, entries = cluster.entry_ids.len(), "page created");
                }
                Err(e) => {
                    warn!(
                        tags = ?cluster.shared_tags,
                        error = %e,
                        "failed to create page"
                    );
                }
            }
        }

        if stats.pages_created > 0 || stats.pages_updated > 0 {
            info!(
                created = stats.pages_created,
                updated = stats.pages_updated,
                skipped = stats.clusters_skipped,
                "wiki synthesis complete"
            );
        }

        Ok(stats)
    }

    async fn create_page(
        &self,
        entry_ids: &[String],
        _shared_tags: &[String],
    ) -> anyhow::Result<String> {
        let entries_text = self.format_entries(entry_ids).await?;

        let messages = vec![
            ChatMessage::text(ChatRole::System, SYNTHESIS_PROMPT),
            ChatMessage::text(
                ChatRole::User,
                format!("Create a wiki page from these memory entries:\n\n{entries_text}"),
            ),
        ];

        let response = self.provider.chat(&messages, &[]).await?;
        let parsed = parse_page_response(&response.content)?;

        let mut page = WikiPage {
            id: String::new(),
            title: parsed.title,
            slug: parsed.slug,
            tags: parsed.tags,
            source_ids: entry_ids.to_vec(),
            created_at: Default::default(),
            updated_at: Default::default(),
            revision: 0,
        };

        self.page_store
            .upsert(&mut page, &parsed.content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(page.slug)
    }

    async fn update_page(
        &self,
        page_id: &str,
        new_entry_ids: &[String],
    ) -> anyhow::Result<()> {
        let (mut page, existing_content) = self
            .page_store
            .get(page_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let new_entries_text = self.format_entries(new_entry_ids).await?;

        let messages = vec![
            ChatMessage::text(ChatRole::System, UPDATE_PROMPT),
            ChatMessage::text(
                ChatRole::User,
                format!(
                    "Existing page:\n\n{existing_content}\n\n---\n\nNew entries to merge:\n\n{new_entries_text}"
                ),
            ),
        ];

        let response = self.provider.chat(&messages, &[]).await?;
        let parsed = parse_page_response(&response.content)?;

        // Merge source_ids
        let mut all_sources: Vec<String> = page.source_ids.clone();
        for id in new_entry_ids {
            if !all_sources.contains(id) {
                all_sources.push(id.clone());
            }
        }
        page.source_ids = all_sources;
        page.title = parsed.title;
        page.tags = parsed.tags;

        self.page_store
            .upsert(&mut page, &parsed.content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(())
    }

    async fn format_entries(&self, entry_ids: &[String]) -> anyhow::Result<String> {
        let mut text = String::new();
        for id in entry_ids {
            match self.memory_store.get(id).await {
                Ok((entry, content)) => {
                    let tags = entry.tags.join(", ");
                    text.push_str(&format!(
                        "[{}] {} ({}, tags: {})\n{}\n\n",
                        entry.id,
                        entry.title,
                        entry.memory_type.as_str(),
                        tags,
                        content,
                    ));
                }
                Err(e) => {
                    warn!(id = %id, error = %e, "skipping unavailable entry");
                }
            }
        }
        Ok(text)
    }
}

// ---- Clustering ----

/// A cluster of entries sharing common tags.
#[derive(Debug)]
struct Cluster {
    entry_ids: Vec<String>,
    shared_tags: Vec<String>,
}

/// Groups entries by tag affinity using a simple union-find on shared tags.
fn cluster_by_tags(entries: &[ozzie_memory::MemoryEntry]) -> Vec<Cluster> {
    // Build tag → entry_ids mapping
    let mut tag_entries: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for tag in &entry.tags {
            let tag = tag.to_lowercase();
            tag_entries.entry(tag).or_default().push(i);
        }
    }

    // Union-Find
    let n = entries.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // For each pair of entries, count shared tags. If >= MIN_SHARED_TAGS, union them.
    for (i, entry_a) in entries.iter().enumerate() {
        let tags_a: HashSet<String> = entry_a.tags.iter().map(|t| t.to_lowercase()).collect();
        for (j, entry_b) in entries.iter().enumerate().skip(i + 1) {
            let shared: usize = entry_b
                .tags
                .iter()
                .filter(|t| tags_a.contains(&t.to_lowercase()))
                .count();
            if shared >= MIN_SHARED_TAGS {
                union(&mut parent, i, j);
            }
        }
    }

    // Collect clusters
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    groups
        .into_values()
        .map(|indices| {
            let entry_ids: Vec<String> = indices.iter().map(|&i| entries[i].id.clone()).collect();

            // Compute truly shared tags (present in all entries of the cluster)
            let tag_sets: Vec<HashSet<String>> = indices
                .iter()
                .map(|&i| entries[i].tags.iter().map(|t| t.to_lowercase()).collect())
                .collect();
            let shared_tags: Vec<String> = if let Some(first) = tag_sets.first() {
                first
                    .iter()
                    .filter(|tag| tag_sets.iter().all(|s| s.contains(*tag)))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };

            Cluster {
                entry_ids,
                shared_tags,
            }
        })
        .collect()
}

// ---- Page coverage helpers ----

/// Maps entry_id → page_id for all existing pages.
fn build_source_map(pages: &[WikiPage]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for page in pages {
        for src in &page.source_ids {
            map.insert(src.clone(), page.id.clone());
        }
    }
    map
}

/// Partitions entry IDs into covered (already in a page) and uncovered.
fn partition_coverage(
    entry_ids: &[String],
    source_map: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut covered = Vec::new();
    let mut uncovered = Vec::new();
    for id in entry_ids {
        if source_map.contains_key(id) {
            covered.push(id.clone());
        } else {
            uncovered.push(id.clone());
        }
    }
    (covered, uncovered)
}

/// Finds the page that covers the most entries in this cluster.
fn find_best_page_for_cluster(
    entry_ids: &[String],
    source_map: &HashMap<String, String>,
) -> Option<String> {
    let mut page_counts: HashMap<&str, usize> = HashMap::new();
    for id in entry_ids {
        if let Some(page_id) = source_map.get(id) {
            *page_counts.entry(page_id.as_str()).or_default() += 1;
        }
    }
    page_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(page_id, _)| page_id.to_string())
}

// ---- Response parsing ----

#[derive(serde::Deserialize)]
struct PageResponse {
    title: String,
    slug: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

fn parse_page_response(raw: &str) -> anyhow::Result<PageResponse> {
    if let Ok(resp) = serde_json::from_str::<PageResponse>(raw) {
        return Ok(resp);
    }

    // Strip markdown fences
    let stripped = extract_json(raw);
    if let Ok(resp) = serde_json::from_str::<PageResponse>(stripped) {
        return Ok(resp);
    }

    anyhow::bail!("failed to parse page synthesis response")
}

fn extract_json(s: &str) -> &str {
    let s = s.trim();
    if let Some(start) = s.find("```json") {
        let after = &s[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = s.find("```") {
        let after = &s[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        return &s[start..=end];
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_memory::{ImportanceLevel, MemoryEntry, MemoryType};

    fn make_entry(id: &str, title: &str, tags: &[&str]) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            title: title.to_string(),
            source: "test".to_string(),
            memory_type: MemoryType::Fact,
            tags: tags.iter().map(|t| t.to_string()).collect(),
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

    #[test]
    fn cluster_groups_by_shared_tags() {
        let entries = vec![
            make_entry("m1", "Rust Error Handling", &["rust", "errors", "thiserror"]),
            make_entry("m2", "Rust Logging", &["rust", "logging", "tracing"]),
            make_entry("m3", "Rust Testing", &["rust", "testing", "errors"]),
            make_entry("m4", "Python Flask", &["python", "flask", "web"]),
            make_entry("m5", "Python Django", &["python", "django", "web"]),
            make_entry("m6", "Python Testing", &["python", "testing", "web"]),
        ];

        let clusters = cluster_by_tags(&entries);

        // Should get 2 clusters: rust-related and python-related
        // (entries linked by 2+ shared tags)
        assert!(clusters.len() >= 2, "expected at least 2 clusters, got {}", clusters.len());

        // Verify no cluster mixes rust and python
        for cluster in &clusters {
            let has_rust = cluster.entry_ids.iter().any(|id| id.starts_with("m1") || id.starts_with("m2") || id.starts_with("m3"));
            let has_python = cluster.entry_ids.iter().any(|id| id.starts_with("m4") || id.starts_with("m5") || id.starts_with("m6"));
            assert!(
                !(has_rust && has_python),
                "cluster should not mix rust and python entries"
            );
        }
    }

    #[test]
    fn cluster_too_few_shared_tags() {
        let entries = vec![
            make_entry("m1", "A", &["rust"]),
            make_entry("m2", "B", &["python"]),
            make_entry("m3", "C", &["go"]),
        ];

        let clusters = cluster_by_tags(&entries);

        // Each entry should be in its own cluster (no shared tags)
        assert_eq!(clusters.len(), 3);
        for cluster in &clusters {
            assert_eq!(cluster.entry_ids.len(), 1);
        }
    }

    #[test]
    fn partition_coverage_works() {
        let mut source_map = HashMap::new();
        source_map.insert("m1".to_string(), "page_a".to_string());
        source_map.insert("m2".to_string(), "page_a".to_string());

        let ids = vec!["m1".to_string(), "m2".to_string(), "m3".to_string()];
        let (covered, uncovered) = partition_coverage(&ids, &source_map);

        assert_eq!(covered, vec!["m1", "m2"]);
        assert_eq!(uncovered, vec!["m3"]);
    }

    #[test]
    fn find_best_page() {
        let mut source_map = HashMap::new();
        source_map.insert("m1".to_string(), "page_a".to_string());
        source_map.insert("m2".to_string(), "page_a".to_string());
        source_map.insert("m3".to_string(), "page_b".to_string());

        let ids = vec!["m1".to_string(), "m2".to_string(), "m3".to_string()];
        let best = find_best_page_for_cluster(&ids, &source_map);
        assert_eq!(best, Some("page_a".to_string()));
    }

    #[test]
    fn parse_clean_json() {
        let json = "{\"title\": \"Rust Patterns\", \"slug\": \"rust-patterns\", \"content\": \"Use thiserror.\", \"tags\": [\"rust\"]}";
        let parsed = parse_page_response(json).unwrap();
        assert_eq!(parsed.title, "Rust Patterns");
        assert_eq!(parsed.slug, "rust-patterns");
        assert!(!parsed.content.is_empty());
    }

    #[test]
    fn parse_with_fences() {
        let raw = "```json\n{\"title\": \"T\", \"slug\": \"t\", \"content\": \"c\", \"tags\": []}\n```";
        let parsed = parse_page_response(raw).unwrap();
        assert_eq!(parsed.title, "T");
    }

    #[test]
    fn parse_garbage_fails() {
        assert!(parse_page_response("not json").is_err());
    }
}
