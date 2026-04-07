use std::collections::HashMap;
use std::sync::RwLock;

use ozzie_core::domain::Message;

// Re-export domain types so existing consumers don't break.
pub use ozzie_core::domain::{Session, SessionError, SessionStatus, SessionStore, SessionTokenUsage};

/// Simple in-memory session store for testing and single-process use.
pub struct InMemorySessionStore {
    sessions: RwLock<HashMap<String, Session>>,
    messages: RwLock<HashMap<String, Vec<Message>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SessionStore for InMemorySessionStore {
    async fn create(&self, session: &Session) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Session>, SessionError> {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        Ok(sessions.get(id).cloned())
    }

    async fn update(&self, session: &Session) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        if !sessions.contains_key(&session.id) {
            return Err(SessionError::NotFound(session.id.clone()));
        }
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        sessions.remove(id);
        let mut messages = self.messages.write().unwrap_or_else(|e| e.into_inner());
        messages.remove(id);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Session>, SessionError> {
        let sessions = self.sessions.read().unwrap_or_else(|e| e.into_inner());
        Ok(sessions.values().cloned().collect())
    }

    async fn append_message(&self, session_id: &str, msg: Message) -> Result<(), SessionError> {
        let mut messages = self.messages.write().unwrap_or_else(|e| e.into_inner());
        messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg);
        Ok(())
    }

    async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>, SessionError> {
        let messages = self.messages.read().unwrap_or_else(|e| e.into_inner());
        Ok(messages.get(session_id).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn create_and_get_session() {
        let store = InMemorySessionStore::new();
        let session = Session::new("s1");

        store.create(&session).await.unwrap();
        let got = store.get("s1").await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().id, "s1");
    }

    #[tokio::test]
    async fn append_and_load_messages() {
        let store = InMemorySessionStore::new();
        let session = Session::new("s1");
        store.create(&session).await.unwrap();

        store
            .append_message("s1", Message::user("hello"))
            .await
            .unwrap();
        store
            .append_message("s1", Message::assistant("hi there"))
            .await
            .unwrap();

        let messages = store.load_messages("s1").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn delete_session() {
        let store = InMemorySessionStore::new();
        let session = Session::new("s1");
        store.create(&session).await.unwrap();
        store.delete("s1").await.unwrap();
        assert!(store.get("s1").await.unwrap().is_none());
    }

    #[test]
    fn session_new_defaults() {
        let s = Session::new("test_id");
        assert_eq!(s.id, "test_id");
        assert_eq!(s.status, SessionStatus::Active);
        assert!(s.model.is_none());
        assert_eq!(s.message_count, 0);
        assert!(s.metadata.is_empty());
        assert!(s.is_active());
    }

    #[test]
    fn session_status_serde() {
        let s = Session::new("serde_test");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""status":"active""#));

        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, SessionStatus::Active);
    }

    #[test]
    fn session_status_closed_serde() {
        let mut s = Session::new("closed_test");
        s.status = SessionStatus::Closed;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""status":"closed""#));

        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, SessionStatus::Closed);
        assert!(!parsed.is_active());
    }

    #[test]
    fn session_model_field() {
        let mut s = Session::new("model_test");
        s.model = Some("claude-3-opus".to_string());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("claude-3-opus"));

        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("claude-3-opus"));
    }

    #[test]
    fn session_message_count() {
        let mut s = Session::new("count_test");
        assert_eq!(s.message_count, 0);
        s.message_count = 42;

        let json = serde_json::to_string(&s).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message_count, 42);
    }

    #[test]
    fn session_metadata() {
        let mut s = Session::new("meta_test");
        s.metadata.insert("connector".to_string(), "discord".to_string());
        s.metadata.insert("user_id".to_string(), "u123".to_string());

        let json = serde_json::to_string(&s).unwrap();
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metadata.get("connector").unwrap(), "discord");
        assert_eq!(parsed.metadata.get("user_id").unwrap(), "u123");
    }

    #[test]
    fn session_backward_compat_missing_fields() {
        let json = r#"{
            "id": "old_session",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;
        let s: Session = serde_json::from_str(json).unwrap();
        assert_eq!(s.id, "old_session");
        assert_eq!(s.status, SessionStatus::Active);
        assert!(s.model.is_none());
        assert_eq!(s.message_count, 0);
        assert!(s.metadata.is_empty());
        assert!(s.approved_tools.is_empty());
    }

    #[test]
    fn session_status_display() {
        assert_eq!(SessionStatus::Active.to_string(), "active");
        assert_eq!(SessionStatus::Closed.to_string(), "closed");
    }

    #[tokio::test]
    async fn close_session() {
        let store = InMemorySessionStore::new();
        let session = Session::new("s_close");
        store.create(&session).await.unwrap();

        store.close("s_close").await.unwrap();

        let got = store.get("s_close").await.unwrap().unwrap();
        assert_eq!(got.status, SessionStatus::Closed);
        assert!(!got.is_active());
    }

    #[tokio::test]
    async fn close_nonexistent_session() {
        let store = InMemorySessionStore::new();
        let result = store.close("nonexistent").await;
        assert!(result.is_err());
    }

    #[test]
    fn metadata_skip_serializing_if_empty() {
        let s = Session::new("empty_meta");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("metadata"));
    }

    #[test]
    fn model_skip_serializing_if_none() {
        let s = Session::new("no_model");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains(r#""model""#));
    }
}
