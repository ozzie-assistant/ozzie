use crate::bm25::BM25;
use crate::estimate_tokens;
use crate::store::ArchiveStore;
use crate::types::{
    Config, Index, Layer, RetrievalDecision, RetrievalResult, Selection, TokenUsage,
};

/// Selects relevant context from the index using BM25 scoring
/// with progressive L0→L1→L2 escalation.
pub struct Retriever<'a> {
    store: &'a dyn ArchiveStore,
    cfg: Config,
}

struct ScoredNode {
    index: usize,
    score: f64,
}

impl<'a> Retriever<'a> {
    pub fn new(store: &'a dyn ArchiveStore, cfg: Config) -> Self {
        Self { store, cfg }
    }

    /// Retrieves the most relevant archived context for the given query.
    pub async fn retrieve(
        &self,
        session_id: &str,
        index: &Index,
        query: &str,
    ) -> RetrievalResult {
        if index.nodes.is_empty() {
            return RetrievalResult::default();
        }

        // Budget: 45% of max prompt tokens, minimum 400
        let budget = ((self.cfg.max_prompt_tokens as f64 * 0.45).floor() as usize).max(400);

        // Build BM25 engine from all node abstracts + keywords
        let docs: Vec<String> = index
            .nodes
            .iter()
            .map(|n| format!("{} {}", n.abstract_text, n.keywords.join(" ")))
            .collect();

        let mut bm = BM25::standard();
        bm.build(&docs);

        // Find max recency rank for normalization
        let max_recency = index
            .nodes
            .iter()
            .map(|n| n.metadata.recency_rank)
            .max()
            .unwrap_or(0);

        // L0 scoring: abstracts + keywords + recency prior
        let mut scored: Vec<ScoredNode> = index
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let mut s = bm.score(query, i);
                // Recency prior: max +0.08 for the most recent
                if max_recency > 0 {
                    s += 0.08 * n.metadata.recency_rank as f64 / max_recency as f64;
                }
                ScoredNode { index: i, score: s }
            })
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let top_score = scored.first().map(|s| s.score).unwrap_or(0.0);

        // Check L0 confidence
        if top_score >= self.cfg.score_threshold_high {
            let decision = RetrievalDecision {
                reached_layer: Some(Layer::L0),
                top_score,
                reason: "high confidence at L0".to_string(),
            };
            let selections = self.select_with_budget(&scored, &index.nodes, budget, Layer::L0, 3);
            return self.build_result(selections, decision, budget);
        }

        // L1 escalation: re-score using summaries
        let top_n = self.cfg.max_items_l1.min(scored.len());
        let candidates = &scored[..top_n];

        let summary_docs: Vec<String> = candidates
            .iter()
            .map(|sn| {
                let n = &index.nodes[sn.index];
                format!("{} {}", n.summary, n.keywords.join(" "))
            })
            .collect();

        let mut bm_l1 = BM25::standard();
        bm_l1.build(&summary_docs);

        let mut l1_scored: Vec<ScoredNode> = candidates
            .iter()
            .enumerate()
            .map(|(local_i, sn)| {
                let n = &index.nodes[sn.index];
                let mut s = bm_l1.score(query, local_i);
                if max_recency > 0 {
                    s += 0.08 * n.metadata.recency_rank as f64 / max_recency as f64;
                }
                ScoredNode {
                    index: sn.index,
                    score: s,
                }
            })
            .collect();

        l1_scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let top_score_l1 = l1_scored.first().map(|s| s.score).unwrap_or(0.0);
        let margin = if l1_scored.len() > 1 {
            l1_scored[0].score - l1_scored[1].score
        } else {
            0.0
        };

        if top_score_l1 >= self.cfg.score_threshold_high || margin >= self.cfg.top1_top2_margin {
            let decision = RetrievalDecision {
                reached_layer: Some(Layer::L1),
                top_score: top_score_l1,
                reason: format!(
                    "L1 confidence: score={:.3} margin={:.3}",
                    top_score_l1, margin
                ),
            };
            let selections = self.select_with_budget(
                &l1_scored,
                &index.nodes,
                budget,
                Layer::L1,
                self.cfg.max_items_l1,
            );
            return self.build_result(selections, decision, budget);
        }

        // L2 escalation: load full transcripts
        let decision = RetrievalDecision {
            reached_layer: Some(Layer::L2),
            top_score: top_score_l1,
            reason: "escalated to L2 full transcripts".to_string(),
        };

        let top_l2 = self.cfg.max_items_l2.min(l1_scored.len());
        let mut selections = Vec::new();
        let mut used_tokens = 0;

        // Always include root abstract first
        let root_abstract = &index.root.abstract_text;
        let root_tokens = estimate_tokens(root_abstract);
        if root_tokens <= budget {
            selections.push(Selection {
                node_id: "root".to_string(),
                layer: Layer::L0,
                content: root_abstract.clone(),
                tokens: root_tokens,
                score: 1.0,
            });
            used_tokens += root_tokens;
        }

        for sn in l1_scored.iter().take(top_l2) {
            let node = &index.nodes[sn.index];

            match self.store.read_archive(session_id, &node.id).await {
                Ok(Some(archive)) => {
                    let tokens = estimate_tokens(&archive.transcript);
                    if used_tokens + tokens <= budget {
                        selections.push(Selection {
                            node_id: node.id.clone(),
                            layer: Layer::L2,
                            content: archive.transcript,
                            tokens,
                            score: sn.score,
                        });
                        used_tokens += tokens;
                    } else {
                        // Too large → fall back to summary
                        let tokens = estimate_tokens(&node.summary);
                        if used_tokens + tokens <= budget {
                            selections.push(Selection {
                                node_id: node.id.clone(),
                                layer: Layer::L1,
                                content: node.summary.clone(),
                                tokens,
                                score: sn.score,
                            });
                            used_tokens += tokens;
                        }
                    }
                }
                _ => {
                    // Archive not found → fall back to summary
                    let tokens = estimate_tokens(&node.summary);
                    if used_tokens + tokens <= budget {
                        selections.push(Selection {
                            node_id: node.id.clone(),
                            layer: Layer::L1,
                            content: node.summary.clone(),
                            tokens,
                            score: sn.score,
                        });
                        used_tokens += tokens;
                    }
                }
            }
        }

        self.build_result(selections, decision, budget)
    }

    /// Greedily picks items at the given layer until budget exhausted.
    fn select_with_budget(
        &self,
        scored: &[ScoredNode],
        nodes: &[crate::types::Node],
        budget: usize,
        layer: Layer,
        max_items: usize,
    ) -> Vec<Selection> {
        let mut selections = Vec::new();
        let mut used_tokens = 0;

        for sn in scored.iter().take(max_items) {
            let node = &nodes[sn.index];
            let content = match layer {
                Layer::L0 => &node.abstract_text,
                Layer::L1 | Layer::L2 => &node.summary,
            };

            let tokens = estimate_tokens(content);
            if used_tokens + tokens > budget {
                continue;
            }

            selections.push(Selection {
                node_id: node.id.clone(),
                layer,
                content: content.clone(),
                tokens,
                score: sn.score,
            });
            used_tokens += tokens;
        }
        selections
    }

    fn build_result(
        &self,
        selections: Vec<Selection>,
        decision: RetrievalDecision,
        budget: usize,
    ) -> RetrievalResult {
        let used: usize = selections.iter().map(|s| s.tokens).sum();
        let savings = if budget > 0 {
            1.0 - used as f64 / budget as f64
        } else {
            0.0
        };

        RetrievalResult {
            selections,
            decision,
            token_usage: TokenUsage {
                budget,
                used,
                savings_ratio: savings,
            },
        }
    }
}

// Integration tests that require a concrete ArchiveStore implementation
// live in ozzie-runtime::layered_store (co-located with the FileArchiveStore).
