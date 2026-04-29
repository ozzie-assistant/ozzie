use std::collections::HashMap;
use std::sync::RwLock;

use ozzie_core::domain::Message;

// Re-export domain types so existing consumers don't break.
pub use ozzie_core::domain::{
    Conversation, ConversationError, ConversationStatus, ConversationStore, ConversationTokenUsage,
};

/// Simple in-memory conversation store for testing and single-process use.
pub struct InMemoryConversationStore {
    conversations: RwLock<HashMap<String, Conversation>>,
    messages: RwLock<HashMap<String, Vec<Message>>>,
}

impl InMemoryConversationStore {
    pub fn new() -> Self {
        Self {
            conversations: RwLock::new(HashMap::new()),
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryConversationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ConversationStore for InMemoryConversationStore {
    async fn create(&self, conversation: &Conversation) -> Result<(), ConversationError> {
        let mut conversations = self.conversations.write().unwrap_or_else(|e| e.into_inner());
        conversations.insert(conversation.id.clone(), conversation.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Conversation>, ConversationError> {
        let conversations = self.conversations.read().unwrap_or_else(|e| e.into_inner());
        Ok(conversations.get(id).cloned())
    }

    async fn update(&self, conversation: &Conversation) -> Result<(), ConversationError> {
        let mut conversations = self.conversations.write().unwrap_or_else(|e| e.into_inner());
        if !conversations.contains_key(&conversation.id) {
            return Err(ConversationError::NotFound(conversation.id.clone()));
        }
        conversations.insert(conversation.id.clone(), conversation.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), ConversationError> {
        let mut conversations = self.conversations.write().unwrap_or_else(|e| e.into_inner());
        conversations.remove(id);
        let mut messages = self.messages.write().unwrap_or_else(|e| e.into_inner());
        messages.remove(id);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Conversation>, ConversationError> {
        let conversations = self.conversations.read().unwrap_or_else(|e| e.into_inner());
        Ok(conversations.values().cloned().collect())
    }

    async fn append_message(
        &self,
        conversation_id: &str,
        msg: Message,
    ) -> Result<(), ConversationError> {
        let mut messages = self.messages.write().unwrap_or_else(|e| e.into_inner());
        messages
            .entry(conversation_id.to_string())
            .or_default()
            .push(msg);
        Ok(())
    }

    async fn load_messages(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<Message>, ConversationError> {
        let messages = self.messages.read().unwrap_or_else(|e| e.into_inner());
        Ok(messages.get(conversation_id).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn create_and_get_conversation() {
        let store = InMemoryConversationStore::new();
        let conversation = Conversation::new("s1");

        store.create(&conversation).await.unwrap();
        let got = store.get("s1").await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().id, "s1");
    }

    #[tokio::test]
    async fn append_and_load_messages() {
        let store = InMemoryConversationStore::new();
        let conversation = Conversation::new("s1");
        store.create(&conversation).await.unwrap();

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
    async fn delete_conversation() {
        let store = InMemoryConversationStore::new();
        let conversation = Conversation::new("s1");
        store.create(&conversation).await.unwrap();
        store.delete("s1").await.unwrap();
        assert!(store.get("s1").await.unwrap().is_none());
    }

    #[test]
    fn conversation_new_defaults() {
        let s = Conversation::new("test_id");
        assert_eq!(s.id, "test_id");
        assert_eq!(s.status, ConversationStatus::Active);
        assert!(s.model.is_none());
        assert_eq!(s.message_count, 0);
        assert!(s.metadata.is_empty());
        assert!(s.is_active());
    }

    #[test]
    fn conversation_status_serde() {
        let s = Conversation::new("serde_test");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""status":"active""#));

        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, ConversationStatus::Active);
    }

    #[test]
    fn conversation_status_archived_serde() {
        let mut s = Conversation::new("archived_test");
        s.status = ConversationStatus::Archived;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""status":"archived""#));

        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, ConversationStatus::Archived);
        assert!(!parsed.is_active());
    }

    #[test]
    fn conversation_model_field() {
        let mut s = Conversation::new("model_test");
        s.model = Some("claude-3-opus".to_string());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("claude-3-opus"));

        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("claude-3-opus"));
    }

    #[test]
    fn conversation_message_count() {
        let mut s = Conversation::new("count_test");
        assert_eq!(s.message_count, 0);
        s.message_count = 42;

        let json = serde_json::to_string(&s).unwrap();
        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message_count, 42);
    }

    #[test]
    fn conversation_metadata() {
        let mut s = Conversation::new("meta_test");
        s.metadata.insert("connector".to_string(), "discord".to_string());
        s.metadata.insert("user_id".to_string(), "u123".to_string());

        let json = serde_json::to_string(&s).unwrap();
        let parsed: Conversation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metadata.get("connector").unwrap(), "discord");
        assert_eq!(parsed.metadata.get("user_id").unwrap(), "u123");
    }

    #[test]
    fn conversation_backward_compat_missing_fields() {
        let json = r#"{
            "id": "old_conv",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;
        let s: Conversation = serde_json::from_str(json).unwrap();
        assert_eq!(s.id, "old_conv");
        assert_eq!(s.status, ConversationStatus::Active);
        assert!(s.model.is_none());
        assert_eq!(s.message_count, 0);
        assert!(s.metadata.is_empty());
        assert!(s.approved_tools.is_empty());
    }

    #[test]
    fn conversation_status_display() {
        assert_eq!(ConversationStatus::Active.to_string(), "active");
        assert_eq!(ConversationStatus::Archived.to_string(), "archived");
    }

    #[tokio::test]
    async fn archive_conversation() {
        let store = InMemoryConversationStore::new();
        let conversation = Conversation::new("s_archive");
        store.create(&conversation).await.unwrap();

        store.archive("s_archive").await.unwrap();

        let got = store.get("s_archive").await.unwrap().unwrap();
        assert_eq!(got.status, ConversationStatus::Archived);
        assert!(!got.is_active());
    }

    #[tokio::test]
    async fn archive_nonexistent_conversation() {
        let store = InMemoryConversationStore::new();
        let result = store.archive("nonexistent").await;
        assert!(result.is_err());
    }

    #[test]
    fn metadata_skip_serializing_if_empty() {
        let s = Conversation::new("empty_meta");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("metadata"));
    }

    #[test]
    fn model_skip_serializing_if_none() {
        let s = Conversation::new("no_model");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains(r#""model""#));
    }
}
