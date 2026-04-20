use std::fmt::Write;

use std::sync::Arc;

use crate::indexer::{Indexer, Summarizer};
use crate::message::{Message, ROLE_ASSISTANT, ROLE_USER};
use crate::retriever::Retriever;
use crate::store::ArchiveStore;
use crate::types::{ApplyResult, Config, RetrievalResult, Selection};

/// Orchestrates the full layered context pipeline:
/// indexer → retriever → compressed message list.
pub struct Manager {
    indexer: Indexer,
    cfg: Config,
}

impl Manager {
    /// Creates a Manager with the given store, config, and summarizer.
    pub fn new(store: Box<dyn ArchiveStore>, cfg: Config, summarizer: Arc<dyn Summarizer>) -> Self {
        Self {
            indexer: Indexer::new(store, summarizer, cfg.clone()),
            cfg,
        }
    }

    /// Runs the full layered context pipeline and returns compressed messages.
    ///
    /// If the history is short enough (`<= max_recent_messages`), returns
    /// the original messages unchanged.
    ///
    /// Otherwise:
    /// 1. Splits history into archived + recent
    /// 2. Builds/updates the index from archived messages
    /// 3. Retrieves relevant context via BM25 scoring
    /// 4. Returns `[context_message, ...recent_messages]`
    pub async fn apply(
        &self,
        session_id: &str,
        history: &[Message],
    ) -> Result<(Vec<Message>, Option<ApplyResult>), ManagerError> {
        // Not enough messages to warrant compression
        if history.len() <= self.cfg.max_recent_messages {
            return Ok((history.to_vec(), None));
        }

        // Split history: archived + recent
        let split_point = history.len() - self.cfg.max_recent_messages;
        let archived = &history[..split_point];
        let recent = &history[split_point..];

        // Build or update the index
        let index = self
            .indexer
            .build_or_update(session_id, archived)
            .await
            .map_err(ManagerError::Indexer)?;

        // Extract query from the last user message
        let query = last_user_message_content(history);

        // Retrieve relevant context
        let result = Retriever::new(self.indexer.store(), self.cfg.clone())
            .retrieve(session_id, &index, &query)
            .await;

        // Build the layered context message
        let context_msg = build_context_message(&result);

        // Filter recent messages (skip empty non-assistant)
        let recent_msgs: Vec<Message> = recent
            .iter()
            .filter(|m| !m.content.is_empty() || m.role == ROLE_ASSISTANT)
            .cloned()
            .collect();

        // Assemble: [context message, ...recent messages]
        let mut out = Vec::with_capacity(1 + recent_msgs.len());
        if let Some(ctx) = context_msg {
            out.push(ctx);
        }
        out.extend(recent_msgs);

        // Build stats
        let escalation = result
            .decision
            .reached_layer
            .map(|l| l.to_string())
            .unwrap_or_default();

        let ar = ApplyResult {
            escalation,
            nodes: result.selections.len(),
            tokens: result.token_usage.used,
            savings_ratio: result.token_usage.savings_ratio,
        };

        Ok((out, Some(ar)))
    }
}

/// Extracts the content of the last user message.
fn last_user_message_content(messages: &[Message]) -> String {
    for msg in messages.iter().rev() {
        if msg.role == ROLE_USER {
            return msg.content.clone();
        }
    }
    String::new()
}

/// Formats retrieved selections into a single context message.
fn build_context_message(result: &RetrievalResult) -> Option<Message> {
    if result.selections.is_empty() {
        return None;
    }

    let mut sb = String::new();
    sb.push_str("[Layered conversation context — retrieved from archived history]\n\n");

    for (i, sel) in result.selections.iter().enumerate() {
        if i > 0 {
            sb.push_str("\n---\n\n");
        }
        let _ = write!(
            sb,
            "### Archive {} ({}, relevance: {:.2})\n\n",
            sel.node_id, sel.layer, sel.score
        );
        sb.push_str(&sel.content);
        sb.push('\n');
    }

    Some(Message::user(sb))
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({}, {}, score={:.3})",
            self.node_id, self.layer, self.tokens, self.score
        )
    }
}

/// Errors from manager operations.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("indexer: {0}")]
    Indexer(#[from] crate::indexer::IndexerError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests (short_history_unchanged, long_history_compressed, apply_creates_index)
    // that require a concrete ArchiveStore live in ozzie-runtime::layered_store.

    #[test]
    fn context_message_format() {
        let result = RetrievalResult {
            selections: vec![Selection {
                node_id: "abc123".to_string(),
                layer: crate::types::Layer::L0,
                content: "test abstract content".to_string(),
                tokens: 10,
                score: 0.85,
            }],
            ..Default::default()
        };

        let msg = build_context_message(&result).unwrap();
        assert_eq!(msg.role, ROLE_USER);
        assert!(msg.content.contains("Layered conversation context"));
        assert!(msg.content.contains("abc123"));
        assert!(msg.content.contains("L0"));
        assert!(msg.content.contains("0.85"));
    }

    #[test]
    fn context_message_empty_selections() {
        let result = RetrievalResult::default();
        assert!(build_context_message(&result).is_none());
    }

    #[test]
    fn last_user_message() {
        let msgs = vec![
            Message::user("first"),
            Message::assistant("reply"),
            Message::user("second"),
        ];
        assert_eq!(last_user_message_content(&msgs), "second");
    }

    #[test]
    fn last_user_message_empty() {
        let msgs: Vec<Message> = vec![];
        assert_eq!(last_user_message_content(&msgs), "");
    }
}
