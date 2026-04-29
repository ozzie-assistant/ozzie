use std::path::{Path, PathBuf};

use ozzie_core::domain::Message;

use crate::conversation::{Conversation, ConversationError, ConversationStore};

/// File-based conversation persistence.
///
/// Layout:
/// ```text
/// base_dir/
///   conv_cosmic_asimov/
///     meta.json       — Conversation metadata
///     messages.jsonl   — Append-only message log
/// ```
pub struct FileConversationStore {
    base_dir: PathBuf,
}

impl FileConversationStore {
    pub fn new(base_dir: &Path) -> Result<Self, ConversationError> {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| ConversationError::Other(format!("create conversation dir: {e}")))?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    fn conversation_dir(&self, id: &str) -> PathBuf {
        self.base_dir.join(id)
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.conversation_dir(id).join("meta.json")
    }

    fn messages_path(&self, id: &str) -> PathBuf {
        self.conversation_dir(id).join("messages.jsonl")
    }

    /// Lists all conversation IDs by scanning directories.
    pub fn list_ids(&self) -> Result<Vec<String>, ConversationError> {
        let mut ids = Vec::new();
        let entries = std::fs::read_dir(&self.base_dir)
            .map_err(|e| ConversationError::Other(format!("read dir: {e}")))?;

        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                ids.push(name.to_string());
            }
        }

        ids.sort();
        Ok(ids)
    }
}

#[async_trait::async_trait]
impl ConversationStore for FileConversationStore {
    async fn create(&self, conversation: &Conversation) -> Result<(), ConversationError> {
        let dir = self.conversation_dir(&conversation.id);
        std::fs::create_dir_all(&dir)
            .map_err(|e| ConversationError::Other(format!("create conversation dir: {e}")))?;

        let json = serde_json::to_string_pretty(conversation)
            .map_err(|e| ConversationError::Other(format!("serialize conversation: {e}")))?;
        std::fs::write(self.meta_path(&conversation.id), json)
            .map_err(|e| ConversationError::Other(format!("write meta: {e}")))?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Conversation>, ConversationError> {
        let path = self.meta_path(id);
        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read_to_string(&path)
            .map_err(|e| ConversationError::Other(format!("read meta: {e}")))?;
        let conversation: Conversation = serde_json::from_str(&data)
            .map_err(|e| ConversationError::Other(format!("parse meta: {e}")))?;
        Ok(Some(conversation))
    }

    async fn update(&self, conversation: &Conversation) -> Result<(), ConversationError> {
        let path = self.meta_path(&conversation.id);
        if !path.exists() {
            return Err(ConversationError::NotFound(conversation.id.clone()));
        }

        let json = serde_json::to_string_pretty(conversation)
            .map_err(|e| ConversationError::Other(format!("serialize conversation: {e}")))?;

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|e| ConversationError::Other(format!("write tmp: {e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| ConversationError::Other(format!("rename: {e}")))?;

        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), ConversationError> {
        let dir = self.conversation_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| ConversationError::Other(format!("remove conversation dir: {e}")))?;
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Conversation>, ConversationError> {
        let ids = self.list_ids()?;
        let mut conversations = Vec::new();
        for id in ids {
            if let Some(conversation) = self.get(&id).await? {
                conversations.push(conversation);
            }
        }
        Ok(conversations)
    }

    async fn append_message(
        &self,
        conversation_id: &str,
        msg: Message,
    ) -> Result<(), ConversationError> {
        let path = self.messages_path(conversation_id);
        let line = serde_json::to_string(&msg)
            .map_err(|e| ConversationError::Other(format!("serialize message: {e}")))?;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| ConversationError::Other(format!("open messages: {e}")))?;
        writeln!(file, "{line}")
            .map_err(|e| ConversationError::Other(format!("write message: {e}")))?;

        Ok(())
    }

    async fn load_messages(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<Message>, ConversationError> {
        let path = self.messages_path(conversation_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = std::fs::read_to_string(&path)
            .map_err(|e| ConversationError::Other(format!("read messages: {e}")))?;

        let mut messages = Vec::new();
        for line in data.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let msg: Message = serde_json::from_str(line)
                .map_err(|e| ConversationError::Other(format!("parse message: {e}")))?;
            messages.push(msg);
        }

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conversation(id: &str) -> Conversation {
        Conversation::new(id)
    }

    #[tokio::test]
    async fn create_get_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        let conversation = make_conversation("conv_test_one");
        store.create(&conversation).await.unwrap();

        let got = store.get("conv_test_one").await.unwrap().unwrap();
        assert_eq!(got.id, "conv_test_one");
    }

    #[tokio::test]
    async fn update_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        let mut conversation = make_conversation("conv_test_upd");
        store.create(&conversation).await.unwrap();

        conversation.summary = Some("updated summary".to_string());
        store.update(&conversation).await.unwrap();

        let got = store.get("conv_test_upd").await.unwrap().unwrap();
        assert_eq!(got.summary.as_deref(), Some("updated summary"));
    }

    #[tokio::test]
    async fn delete_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        store.create(&make_conversation("conv_del")).await.unwrap();
        store.delete("conv_del").await.unwrap();
        assert!(store.get("conv_del").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        store.create(&make_conversation("conv_a")).await.unwrap();
        store.create(&make_conversation("conv_b")).await.unwrap();

        let conversations = store.list().await.unwrap();
        assert_eq!(conversations.len(), 2);
    }

    #[tokio::test]
    async fn append_and_load_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        store.create(&make_conversation("conv_msg")).await.unwrap();

        store
            .append_message("conv_msg", Message::user("hello"))
            .await
            .unwrap();
        store
            .append_message("conv_msg", Message::assistant("hi"))
            .await
            .unwrap();

        let messages = store.load_messages("conv_msg").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn list_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileConversationStore::new(dir.path()).unwrap();

        store.create(&make_conversation("conv_x")).await.unwrap();
        store.create(&make_conversation("conv_y")).await.unwrap();

        let ids = store.list_ids().unwrap();
        assert_eq!(ids, vec!["conv_x", "conv_y"]);
    }
}
