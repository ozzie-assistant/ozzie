use std::collections::{HashMap, HashSet};

use worm_memory::{MemoryEntry, MemorySchema, WikiPage};

/// Minimum entries sharing 2+ tags to form a cluster.
pub const MIN_CLUSTER_SIZE: usize = 3;
/// Minimum shared tags for two entries to belong to the same cluster.
pub const MIN_SHARED_TAGS: usize = 2;

/// Prompt constants for wiki page synthesis.
pub const BASE_SYNTHESIS_PROMPT: &str = r#"You are creating a wiki-style knowledge page from related memory entries.
Synthesize a coherent, well-structured markdown page that captures all the information.
Use markdown headers (##) for structure. Be comprehensive but not redundant.

Respond with JSON only, no markdown fences:
{"title": "Page Title", "slug": "url-safe-slug", "content": "markdown content...", "tags": ["tag1", "tag2"]}"#;

pub const BASE_UPDATE_PROMPT: &str = r#"You are updating an existing wiki page with new information from memory entries.
Merge the new entries into the existing page, preserving structure and removing redundancy.

Respond with JSON only, no markdown fences:
{"title": "Page Title", "slug": "url-safe-slug", "content": "updated markdown content...", "tags": ["tag1", "tag2"]}"#;

pub const SPLIT_PROMPT: &str = r#"The following wiki page is too long. Split it into 2-3 shorter, self-contained pages.
Each page should cover a coherent sub-topic. Distribute the source memory IDs to the relevant sub-page.

Respond with a JSON array only, no markdown fences:
[{"title": "...", "slug": "...", "content": "...", "tags": [...], "source_ids": ["mem_..."]}]"#;

/// Builds the full synthesis prompt with schema conventions.
pub fn build_synthesis_prompt(schema: &MemorySchema) -> String {
    let mut prompt = BASE_SYNTHESIS_PROMPT.to_string();
    apply_schema_suffix(&mut prompt, schema);
    prompt
}

/// Builds the full update prompt with schema conventions.
pub fn build_update_prompt(schema: &MemorySchema) -> String {
    let mut prompt = BASE_UPDATE_PROMPT.to_string();
    apply_schema_suffix(&mut prompt, schema);
    prompt
}

fn apply_schema_suffix(prompt: &mut String, schema: &MemorySchema) {
    if let Some(ref lang) = schema.language {
        prompt.push_str(&format!("\nWrite in {lang}."));
    } else {
        prompt.push_str("\nWrite in the same language as the source entries.");
    }
    if !schema.instructions.is_empty() {
        prompt.push_str(&format!(
            "\n\nAdditional conventions:\n{}",
            schema.instructions
        ));
    }
}

// ---- Clustering ----

/// A cluster of entries sharing common tags.
#[derive(Debug)]
pub struct Cluster {
    pub entry_ids: Vec<String>,
    pub shared_tags: Vec<String>,
}

/// Groups entries by tag affinity using union-find on shared tags.
pub fn cluster_by_tags(entries: &[MemoryEntry]) -> Vec<Cluster> {
    let mut tag_entries: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for tag in &entry.tags {
            let tag = tag.to_lowercase();
            tag_entries.entry(tag).or_default().push(i);
        }
    }

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

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    groups
        .into_values()
        .map(|indices| {
            let entry_ids: Vec<String> = indices.iter().map(|&i| entries[i].id.clone()).collect();

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
pub fn build_source_map(pages: &[WikiPage]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for page in pages {
        for src in &page.source_ids {
            map.insert(src.clone(), page.id.clone());
        }
    }
    map
}

/// Partitions entry IDs into covered (already in a page) and uncovered.
pub fn partition_coverage(
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
pub fn find_best_page_for_cluster(
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

/// Parsed page response from LLM.
#[derive(Debug, serde::Deserialize)]
pub struct PageResponse {
    pub title: String,
    pub slug: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Parsed split page entry from LLM.
#[derive(Debug, serde::Deserialize)]
pub struct SplitPageEntry {
    pub title: String,
    pub slug: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

pub fn parse_page_response(raw: &str) -> Result<PageResponse, String> {
    if let Ok(resp) = serde_json::from_str::<PageResponse>(raw) {
        return Ok(resp);
    }
    let stripped = crate::classifier::extract_json(raw);
    serde_json::from_str::<PageResponse>(stripped)
        .map_err(|e| format!("failed to parse page synthesis response: {e}"))
}

pub fn parse_split_response(raw: &str) -> Result<Vec<SplitPageEntry>, String> {
    if let Ok(resp) = serde_json::from_str::<Vec<SplitPageEntry>>(raw) {
        return Ok(resp);
    }
    let stripped = crate::classifier::extract_json_array(raw);
    serde_json::from_str::<Vec<SplitPageEntry>>(stripped)
        .map_err(|e| format!("failed to parse split response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use worm_memory::{ImportanceLevel, MemoryType};

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
        assert!(clusters.len() >= 2);

        for cluster in &clusters {
            let has_rust = cluster.entry_ids.iter().any(|id| id.starts_with("m1") || id.starts_with("m2") || id.starts_with("m3"));
            let has_python = cluster.entry_ids.iter().any(|id| id.starts_with("m4") || id.starts_with("m5") || id.starts_with("m6"));
            assert!(!(has_rust && has_python));
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
        assert_eq!(clusters.len(), 3);
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
    fn parse_clean_page_response() {
        let json = r#"{"title": "Rust Patterns", "slug": "rust-patterns", "content": "Use thiserror.", "tags": ["rust"]}"#;
        let parsed = parse_page_response(json).unwrap();
        assert_eq!(parsed.title, "Rust Patterns");
        assert_eq!(parsed.slug, "rust-patterns");
    }

    #[test]
    fn parse_page_with_fences() {
        let raw = "```json\n{\"title\": \"T\", \"slug\": \"t\", \"content\": \"c\", \"tags\": []}\n```";
        let parsed = parse_page_response(raw).unwrap();
        assert_eq!(parsed.title, "T");
    }
}
