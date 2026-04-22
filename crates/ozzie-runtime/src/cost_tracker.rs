use std::sync::Arc;

use tracing::{debug, error};

use ozzie_core::events::{Event, EventBus, EventKind, EventPayload};

use crate::conversation::ConversationStore;

/// Subscribes to LLM call events and accumulates token usage per session.
///
/// Mirrors Go's `sessions.CostTracker`: listens for `internal.llm.call` events
/// with phase="response", extracts `tokens_input` and `tokens_output`, and adds
/// them to the session's cumulative `token_usage`.
pub struct CostTracker {
    bus: Arc<dyn EventBus>,
    store: Arc<dyn ConversationStore>,
}

impl CostTracker {
    /// Creates and starts a CostTracker that listens for LLM response events.
    pub fn new(bus: Arc<dyn EventBus>, store: Arc<dyn ConversationStore>) -> Arc<Self> {
        let tracker = Arc::new(Self {
            bus: bus.clone(),
            store,
        });

        let t = tracker.clone();
        tokio::spawn(async move {
            t.run_loop().await;
        });

        tracker
    }

    async fn run_loop(&self) {
        let mut rx = self.bus.subscribe(&[EventKind::LlmCall.as_str()]);

        loop {
            match rx.recv().await {
                Ok(event) => {
                    self.handle_event(event).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    debug!(skipped = n, "cost tracker lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    debug!("event bus closed, stopping cost tracker");
                    break;
                }
            }
        }
    }

    async fn handle_event(&self, event: Event) {
        let session_id = match &event.session_id {
            Some(sid) if !sid.is_empty() => sid.clone(),
            _ => return,
        };

        // Extract fields from typed payload
        let (phase, tokens_input, tokens_output) = match &event.payload {
            EventPayload::LlmCall {
                phase,
                tokens_input,
                tokens_output,
            } => (phase.as_str(), *tokens_input, *tokens_output),
            _ => return,
        };

        // Only process "response" phase events
        if phase != "response" {
            return;
        }

        // Skip zero-token events (no cost to track)
        if tokens_input == 0 && tokens_output == 0 {
            return;
        }

        // Load session, accumulate, and persist
        let mut session = match self.store.get(&session_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                debug!(session_id = %session_id, "cost tracker: session not found");
                return;
            }
            Err(e) => {
                debug!(session_id = %session_id, error = %e, "cost tracker: failed to load session");
                return;
            }
        };

        session.token_usage.input += tokens_input;
        session.token_usage.output += tokens_output;

        if let Err(e) = self.store.update(&session).await {
            error!(session_id = %session_id, error = %e, "cost tracker: failed to update session");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{InMemoryConversationStore, Conversation};
    use ozzie_core::events::{Bus, EventSource};

    fn publish_llm_event(
        bus: &Arc<dyn EventBus>,
        session_id: &str,
        phase: &str,
        tokens_in: u64,
        tokens_out: u64,
    ) {
        bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::LlmCall {
                phase: phase.to_string(),
                tokens_input: tokens_in,
                tokens_output: tokens_out,
            },
            session_id,
        ));
    }

    #[tokio::test]
    async fn accumulates_token_usage() {
        let bus: Arc<dyn EventBus> = Arc::new(Bus::new(64));
        let store = Arc::new(InMemoryConversationStore::new());

        let session = Conversation::new("sess_cost_1");
        store.create(&session).await.unwrap();

        let _tracker = CostTracker::new(bus.clone(), store.clone() as Arc<dyn ConversationStore>);

        // Give the tracker time to subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        publish_llm_event(&bus, "sess_cost_1", "response", 100, 50);
        publish_llm_event(&bus, "sess_cost_1", "response", 200, 80);

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let got = store.get("sess_cost_1").await.unwrap().unwrap();
        assert_eq!(got.token_usage.input, 300);
        assert_eq!(got.token_usage.output, 130);
    }

    #[tokio::test]
    async fn filters_non_response_phase() {
        let bus: Arc<dyn EventBus> = Arc::new(Bus::new(64));
        let store = Arc::new(InMemoryConversationStore::new());

        let session = Conversation::new("sess_cost_2");
        store.create(&session).await.unwrap();

        let _tracker = CostTracker::new(bus.clone(), store.clone() as Arc<dyn ConversationStore>);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        publish_llm_event(&bus, "sess_cost_2", "request", 100, 0);
        publish_llm_event(&bus, "sess_cost_2", "error", 0, 0);

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let got = store.get("sess_cost_2").await.unwrap().unwrap();
        assert_eq!(got.token_usage.input, 0);
        assert_eq!(got.token_usage.output, 0);
    }

    #[tokio::test]
    async fn ignores_empty_session_id() {
        let bus: Arc<dyn EventBus> = Arc::new(Bus::new(64));
        let store = Arc::new(InMemoryConversationStore::new());

        let _tracker = CostTracker::new(bus.clone(), store.clone() as Arc<dyn ConversationStore>);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Publish without session ID — should not panic
        publish_llm_event(&bus, "", "response", 100, 50);

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        // No assertion needed — just verify no panic
    }

    #[tokio::test]
    async fn ignores_zero_tokens() {
        let bus: Arc<dyn EventBus> = Arc::new(Bus::new(64));
        let store = Arc::new(InMemoryConversationStore::new());

        let session = Conversation::new("sess_cost_3");
        store.create(&session).await.unwrap();

        let _tracker = CostTracker::new(bus.clone(), store.clone() as Arc<dyn ConversationStore>);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        publish_llm_event(&bus, "sess_cost_3", "response", 0, 0);

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let got = store.get("sess_cost_3").await.unwrap().unwrap();
        assert_eq!(got.token_usage.input, 0);
        assert_eq!(got.token_usage.output, 0);
    }

    #[tokio::test]
    async fn ignores_unknown_session() {
        let bus: Arc<dyn EventBus> = Arc::new(Bus::new(64));
        let store = Arc::new(InMemoryConversationStore::new());

        let _tracker = CostTracker::new(bus.clone(), store.clone() as Arc<dyn ConversationStore>);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Publish for a non-existent session — should not panic
        publish_llm_event(&bus, "nonexistent", "response", 100, 50);

        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    #[test]
    fn session_token_usage_serde() {
        let mut s = Conversation::new("serde_cost");
        s.token_usage.input = 500;
        s.token_usage.output = 200;

        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("token_usage"));
        assert!(json.contains("500"));
        assert!(json.contains("200"));

        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.token_usage.input, 500);
        assert_eq!(parsed.token_usage.output, 200);
    }

    #[test]
    fn session_token_usage_skip_if_zero() {
        let s = Conversation::new("zero_cost");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("token_usage"));
    }

    #[test]
    fn session_token_usage_backward_compat() {
        // Simulate loading a session saved before token_usage was added
        let json = r#"{
            "id": "old_session",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;
        let s: Conversation = serde_json::from_str(json).unwrap();
        assert_eq!(s.token_usage.input, 0);
        assert_eq!(s.token_usage.output, 0);
        assert!(s.token_usage.is_zero());
    }
}
