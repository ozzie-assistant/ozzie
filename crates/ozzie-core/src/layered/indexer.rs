use std::collections::HashMap;
use std::fmt::Write;

use chrono::Utc;

use crate::domain::Message;
use crate::layered::keywords::extract_keywords;
use crate::layered::store::ArchiveStore;
use crate::layered::types::{
    ArchivePayload, Config, Index, Node, NodeMetadata, NodeTokenEstimate, Root,
};
use crate::layered::{estimate_tokens, trim_to_tokens};

/// Function signature for summarization: (text, target_tokens) → summary.
/// This is the heuristic fallback; LLM-based summarizers can also implement this.
pub type SummarizerFn = Box<dyn Fn(&str, usize) -> String + Send + Sync>;

/// Builds and incrementally updates the layered index for a session.
pub struct Indexer {
    store: Box<dyn ArchiveStore>,
    summarizer: SummarizerFn,
    cfg: Config,
}

impl Indexer {
    pub fn new(store: Box<dyn ArchiveStore>, summarizer: SummarizerFn, cfg: Config) -> Self {
        Self {
            store,
            summarizer,
            cfg,
        }
    }

    /// Builds or incrementally updates the index for a session.
    pub fn build_or_update(
        &self,
        session_id: &str,
        archived: &[Message],
    ) -> Result<Index, IndexerError> {
        let existing = self.store.load_index(session_id).map_err(IndexerError::Store)?;

        // Build checksum cache from existing nodes
        let checksum_cache: HashMap<String, &Node> = existing
            .as_ref()
            .map(|idx| {
                idx.nodes
                    .iter()
                    .map(|n| (n.checksum.clone(), n))
                    .collect()
            })
            .unwrap_or_default();

        // Chunk messages
        let chunks = chunk_messages(archived, self.cfg.archive_chunk_size);
        let now = Utc::now();

        let mut nodes = Vec::new();
        let num_chunks = chunks.len();

        for (i, chunk) in chunks.iter().enumerate() {
            let transcript = format_transcript(chunk);
            let checksum = compute_checksum(&transcript);

            // Cache hit?
            if let Some(cached) = checksum_cache.get(&checksum) {
                let mut node = (*cached).clone();
                node.metadata.recency_rank = num_chunks - 1 - i;
                node.updated_at = now;
                nodes.push(node);
                continue;
            }

            // Cache miss — generate summaries
            let summary = (self.summarizer)(&transcript, self.cfg.l1_target_tokens);
            let abstract_text = (self.summarizer)(&summary, self.cfg.l0_target_tokens);
            let keywords = extract_keywords(&transcript, 10);

            let node_id_len = 12.min(checksum.len());
            let node_id = checksum[..node_id_len].to_string();
            let node = Node {
                id: node_id.clone(),
                abstract_text: abstract_text.clone(),
                summary: summary.clone(),
                resource_path: format!("archives/archive_{}.json", node_id),
                checksum: checksum.clone(),
                keywords,
                metadata: NodeMetadata {
                    message_count: chunk.len(),
                    recency_rank: num_chunks - 1 - i,
                },
                token_estimate: NodeTokenEstimate {
                    abstract_tokens: estimate_tokens(&abstract_text),
                    summary_tokens: estimate_tokens(&summary),
                    transcript_tokens: estimate_tokens(&transcript),
                },
                created_at: now,
                updated_at: now,
            };
            nodes.push(node.clone());

            // Write archive
            self.store
                .write_archive(
                    session_id,
                    &node_id,
                    &ArchivePayload {
                        node_id: node_id.clone(),
                        transcript,
                    },
                )
                .map_err(IndexerError::Store)?;
        }

        // Trim to MaxArchives (keep most recent)
        if nodes.len() > self.cfg.max_archives {
            nodes = nodes.split_off(nodes.len() - self.cfg.max_archives);
        }

        // Build root
        let root = self.build_root(&nodes);

        // Build index
        let index = Index {
            version: 1,
            session_id: session_id.to_string(),
            root,
            nodes: nodes.clone(),
            created_at: existing
                .as_ref()
                .map(|e| e.created_at)
                .unwrap_or(now),
            updated_at: now,
        };

        // Persist
        self.store
            .save_index(session_id, &index)
            .map_err(IndexerError::Store)?;

        // Cleanup orphaned archives
        let valid_ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
        let _ = self.store.cleanup_archives(session_id, &valid_ids);

        Ok(index)
    }

    /// Returns a reference to the store (for retriever access).
    pub fn store(&self) -> &dyn ArchiveStore {
        &*self.store
    }

    fn build_root(&self, nodes: &[Node]) -> Root {
        let mut all_abstracts = String::new();
        let child_ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();

        for n in nodes {
            all_abstracts.push_str(&n.abstract_text);
            all_abstracts.push('\n');
        }

        let summary = (self.summarizer)(&all_abstracts, self.cfg.l1_target_tokens);
        let abstract_text = (self.summarizer)(&summary, self.cfg.l0_target_tokens);
        let keywords = extract_keywords(&all_abstracts, 15);

        Root {
            id: "root".to_string(),
            abstract_text,
            summary,
            keywords,
            child_ids,
        }
    }
}

/// Chunks messages into groups of `chunk_size`.
fn chunk_messages(messages: &[Message], chunk_size: usize) -> Vec<Vec<Message>> {
    let size = if chunk_size == 0 { 8 } else { chunk_size };
    messages.chunks(size).map(|c| c.to_vec()).collect()
}

/// Formats messages into a readable transcript.
fn format_transcript(messages: &[Message]) -> String {
    let mut sb = String::new();
    for m in messages {
        let _ = writeln!(sb, "[{}]: {}", m.role, m.content);
    }
    sb
}

/// Computes a hex-encoded hash of the transcript for cache validation.
/// Uses a simple FNV-1a-like hash (no external crate needed).
fn compute_checksum(transcript: &str) -> String {
    // FNV-1a 64-bit hash, hex-encoded to 16 chars
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in transcript.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Heuristic fallback summarizer (no LLM required).
///
/// - For small budgets (≤ 150 tokens, L0): first 2 sentences.
/// - For larger budgets (L1): first 18 non-empty lines as bullet list.
pub fn fallback_summarizer(text: &str, target_tokens: usize) -> String {
    let non_empty: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if target_tokens <= 150 {
        // L0 mode: first 2 sentences
        let joined: String = non_empty.join(" ");
        let sentences = split_sentences(&joined);
        let take = sentences.len().min(2);
        let result = sentences[..take].join(" ");
        trim_to_tokens(&result, target_tokens).to_string()
    } else {
        // L1 mode: bullet list of first lines
        let max_lines = 18;
        let take = non_empty.len().min(max_lines);
        let mut sb = String::new();
        for line in &non_empty[..take] {
            let _ = writeln!(sb, "- {line}");
        }
        trim_to_tokens(&sb, target_tokens).to_string()
    }
}

/// Splits text at sentence boundaries (. ! ?).
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            let s = current.trim().to_string();
            if !s.is_empty() {
                sentences.push(s);
            }
            current.clear();
        }
    }
    let s = current.trim().to_string();
    if !s.is_empty() {
        sentences.push(s);
    }
    sentences
}

/// Errors from indexer operations.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("store: {0}")]
    Store(#[from] crate::layered::store::StoreError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_transcript_basic() {
        let msgs = vec![
            Message::user("hello"),
            Message::assistant("hi there"),
        ];
        let transcript = format_transcript(&msgs);
        assert!(transcript.contains("[user]: hello"));
        assert!(transcript.contains("[assistant]: hi there"));
    }

    #[test]
    fn compute_checksum_deterministic() {
        let c1 = compute_checksum("hello world");
        let c2 = compute_checksum("hello world");
        assert_eq!(c1, c2);

        let c3 = compute_checksum("different text");
        assert_ne!(c1, c3);
    }

    #[test]
    fn fallback_summarizer_l0() {
        let text = "First sentence. Second sentence. Third sentence. Fourth sentence.";
        let result = fallback_summarizer(text, 100);
        assert!(result.contains("First sentence."));
        assert!(result.contains("Second sentence."));
        // Should NOT contain third sentence (L0 = 2 sentences max)
        assert!(!result.contains("Third sentence."));
    }

    #[test]
    fn fallback_summarizer_l1() {
        let text = "Line one\nLine two\nLine three\n\nLine five";
        let result = fallback_summarizer(text, 500);
        assert!(result.contains("- Line one"));
        assert!(result.contains("- Line two"));
        assert!(result.contains("- Line three"));
        assert!(result.contains("- Line five"));
    }

    #[test]
    fn split_sentences_basic() {
        let s = split_sentences("Hello world. How are you? I'm fine!");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0], "Hello world.");
        assert_eq!(s[1], "How are you?");
        assert_eq!(s[2], "I'm fine!");
    }

    #[test]
    fn chunk_messages_basic() {
        let msgs: Vec<Message> = (0..10).map(|i| Message::user(format!("msg{i}"))).collect();
        let chunks = chunk_messages(&msgs, 4);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 4);
        assert_eq!(chunks[2].len(), 2);
    }

    // Integration tests that require a concrete ArchiveStore implementation
    // live in ozzie-runtime::layered_store (co-located with the FileArchiveStore).
}
