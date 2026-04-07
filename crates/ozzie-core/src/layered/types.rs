use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for the layered context system.
#[derive(Debug, Clone)]
pub struct Config {
    /// Target tokens for L0 abstracts (default: 120).
    pub l0_target_tokens: usize,
    /// Target tokens for L1 summaries (default: 1200).
    pub l1_target_tokens: usize,
    /// Maximum total prompt tokens (default: 100_000).
    pub max_prompt_tokens: usize,
    /// BM25 score threshold for confident selection (default: 0.64).
    pub score_threshold_high: f64,
    /// Margin between top-1 and top-2 at L1 (default: 0.08).
    pub top1_top2_margin: f64,
    /// Maximum L1 candidates for re-scoring (default: 4).
    pub max_items_l1: usize,
    /// Maximum L2 full transcripts to load (default: 2).
    pub max_items_l2: usize,
    /// Maximum archive nodes in index (default: 12).
    pub max_archives: usize,
    /// Recent messages always kept uncompressed (default: 24).
    pub max_recent_messages: usize,
    /// Messages per archive chunk (default: 8).
    pub archive_chunk_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l0_target_tokens: 120,
            l1_target_tokens: 1200,
            max_prompt_tokens: 100_000,
            score_threshold_high: 0.64,
            top1_top2_margin: 0.08,
            max_items_l1: 4,
            max_items_l2: 2,
            max_archives: 12,
            max_recent_messages: 24,
            archive_chunk_size: 8,
        }
    }
}

/// Which compression layer was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    L0,
    L1,
    L2,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Layer::L0 => write!(f, "L0"),
            Layer::L1 => write!(f, "L1"),
            Layer::L2 => write!(f, "L2"),
        }
    }
}

/// Decision about which retrieval layer was reached.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrievalDecision {
    pub reached_layer: Option<Layer>,
    pub top_score: f64,
    pub reason: String,
}

/// Token usage for a retrieval pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub budget: usize,
    pub used: usize,
    pub savings_ratio: f64,
}

/// Token counts for each layer of a node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeTokenEstimate {
    pub abstract_tokens: usize,
    pub summary_tokens: usize,
    pub transcript_tokens: usize,
}

/// Metadata for a compressed node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub message_count: usize,
    pub recency_rank: usize,
}

/// A compressed archive node in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// L0 ultra-compressed abstract (~120 tokens).
    pub abstract_text: String,
    /// L1 bullet-point summary (~1200 tokens).
    pub summary: String,
    /// Path to full transcript archive on disk.
    pub resource_path: String,
    /// SHA1-like checksum of transcript (for cache invalidation).
    pub checksum: String,
    /// Extracted keywords for BM25.
    pub keywords: Vec<String>,
    pub metadata: NodeMetadata,
    pub token_estimate: NodeTokenEstimate,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Root node summary of the entire session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Root {
    pub id: String,
    pub abstract_text: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub child_ids: Vec<String>,
}

/// The complete session index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    pub session_id: String,
    pub root: Root,
    pub nodes: Vec<Node>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Full transcript payload stored alongside the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivePayload {
    pub node_id: String,
    pub transcript: String,
}

/// A single item selected for context injection.
#[derive(Debug, Clone)]
pub struct Selection {
    pub node_id: String,
    pub layer: Layer,
    pub content: String,
    pub tokens: usize,
    pub score: f64,
}

/// Result of a retrieval operation.
#[derive(Debug, Clone, Default)]
pub struct RetrievalResult {
    pub selections: Vec<Selection>,
    pub decision: RetrievalDecision,
    pub token_usage: TokenUsage,
}


/// Stats from a layered context compression pass.
pub struct ApplyResult {
    /// Deepest layer reached (L0, L1, L2).
    pub escalation: String,
    /// Number of archive nodes selected.
    pub nodes: usize,
    /// Total tokens used by selected context.
    pub tokens: usize,
    /// 1 - (used / budget).
    pub savings_ratio: f64,
}
