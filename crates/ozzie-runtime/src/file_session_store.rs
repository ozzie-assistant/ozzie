use std::path::{Path, PathBuf};

use ozzie_core::domain::Message;

use crate::session::{Session, SessionError, SessionStore};

/// File-based session persistence.
///
/// Layout:
/// ```text
/// base_dir/
///   sess_cosmic_asimov/
///     meta.json       — Session metadata
///     messages.jsonl   — Append-only message log
/// ```
pub struct FileSessionStore {
    base_dir: PathBuf,
}

impl FileSessionStore {
    pub fn new(base_dir: &Path) -> Result<Self, SessionError> {
        std::fs::create_dir_all(base_dir)
            .map_err(|e| SessionError::Other(format!("create session dir: {e}")))?;
        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    fn session_dir(&self, id: &str) -> PathBuf {
        self.base_dir.join(id)
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.session_dir(id).join("meta.json")
    }

    fn messages_path(&self, id: &str) -> PathBuf {
        self.session_dir(id).join("messages.jsonl")
    }

    /// Lists all session IDs by scanning directories.
    pub fn list_ids(&self) -> Result<Vec<String>, SessionError> {
        let mut ids = Vec::new();
        let entries = std::fs::read_dir(&self.base_dir)
            .map_err(|e| SessionError::Other(format!("read dir: {e}")))?;

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
impl SessionStore for FileSessionStore {
    async fn create(&self, session: &Session) -> Result<(), SessionError> {
        let dir = self.session_dir(&session.id);
        std::fs::create_dir_all(&dir)
            .map_err(|e| SessionError::Other(format!("create session dir: {e}")))?;

        let json = serde_json::to_string_pretty(session)
            .map_err(|e| SessionError::Other(format!("serialize session: {e}")))?;
        std::fs::write(self.meta_path(&session.id), json)
            .map_err(|e| SessionError::Other(format!("write meta: {e}")))?;

        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Session>, SessionError> {
        let path = self.meta_path(id);
        if !path.exists() {
            return Ok(None);
        }

        let data = std::fs::read_to_string(&path)
            .map_err(|e| SessionError::Other(format!("read meta: {e}")))?;
        let session: Session = serde_json::from_str(&data)
            .map_err(|e| SessionError::Other(format!("parse meta: {e}")))?;
        Ok(Some(session))
    }

    async fn update(&self, session: &Session) -> Result<(), SessionError> {
        let path = self.meta_path(&session.id);
        if !path.exists() {
            return Err(SessionError::NotFound(session.id.clone()));
        }

        let json = serde_json::to_string_pretty(session)
            .map_err(|e| SessionError::Other(format!("serialize session: {e}")))?;

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|e| SessionError::Other(format!("write tmp: {e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| SessionError::Other(format!("rename: {e}")))?;

        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), SessionError> {
        let dir = self.session_dir(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| SessionError::Other(format!("remove session dir: {e}")))?;
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<Session>, SessionError> {
        let ids = self.list_ids()?;
        let mut sessions = Vec::new();
        for id in ids {
            if let Some(session) = self.get(&id).await? {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    async fn append_message(&self, session_id: &str, msg: Message) -> Result<(), SessionError> {
        let path = self.messages_path(session_id);
        let line = serde_json::to_string(&msg)
            .map_err(|e| SessionError::Other(format!("serialize message: {e}")))?;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| SessionError::Other(format!("open messages: {e}")))?;
        writeln!(file, "{line}")
            .map_err(|e| SessionError::Other(format!("write message: {e}")))?;

        Ok(())
    }

    async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>, SessionError> {
        let path = self.messages_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let data = std::fs::read_to_string(&path)
            .map_err(|e| SessionError::Other(format!("read messages: {e}")))?;

        let mut messages = Vec::new();
        for line in data.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let msg: Message = serde_json::from_str(line)
                .map_err(|e| SessionError::Other(format!("parse message: {e}")))?;
            messages.push(msg);
        }

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str) -> Session {
        Session::new(id)
    }

    #[tokio::test]
    async fn create_get_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        let session = make_session("sess_test_one");
        store.create(&session).await.unwrap();

        let got = store.get("sess_test_one").await.unwrap().unwrap();
        assert_eq!(got.id, "sess_test_one");
    }

    #[tokio::test]
    async fn update_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        let mut session = make_session("sess_test_upd");
        store.create(&session).await.unwrap();

        session.summary = Some("updated summary".to_string());
        store.update(&session).await.unwrap();

        let got = store.get("sess_test_upd").await.unwrap().unwrap();
        assert_eq!(got.summary.as_deref(), Some("updated summary"));
    }

    #[tokio::test]
    async fn delete_session() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        store.create(&make_session("sess_del")).await.unwrap();
        store.delete("sess_del").await.unwrap();
        assert!(store.get("sess_del").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        store.create(&make_session("sess_a")).await.unwrap();
        store.create(&make_session("sess_b")).await.unwrap();

        let sessions = store.list().await.unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn append_and_load_messages() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        store.create(&make_session("sess_msg")).await.unwrap();

        store
            .append_message("sess_msg", Message::user("hello"))
            .await
            .unwrap();
        store
            .append_message("sess_msg", Message::assistant("hi"))
            .await
            .unwrap();

        let messages = store.load_messages("sess_msg").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn list_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSessionStore::new(dir.path()).unwrap();

        store.create(&make_session("sess_x")).await.unwrap();
        store.create(&make_session("sess_y")).await.unwrap();

        let ids = store.list_ids().unwrap();
        assert_eq!(ids, vec!["sess_x", "sess_y"]);
    }
}
