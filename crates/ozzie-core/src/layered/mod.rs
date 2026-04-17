// Re-export from sage-layered — the canonical source of truth.
pub use sage_layered::{
    extract_keywords, chunk_messages, estimate_tokens, fallback_summarizer, trim_to_tokens,
    ApplyResult, ArchivePayload, ArchiveStore, BM25, Config, Index, Indexer, Layer,
    Manager, Node, NodeMetadata, NodeTokenEstimate, RetrievalDecision, RetrievalResult,
    Root, Selection, StoreError, SummarizerFn, TokenUsage,
};

// Re-export submodules for consumers using `layered::store::*` and `layered::retriever::*` paths.
pub use sage_layered::{store, retriever};

/// Convert domain messages to layered messages.
pub fn to_layered_messages(messages: &[crate::domain::Message]) -> Vec<sage_layered::Message> {
    messages
        .iter()
        .map(|m| sage_layered::Message {
            role: m.role.clone(),
            content: m.content.clone(),
            ts: m.ts,
        })
        .collect()
}
