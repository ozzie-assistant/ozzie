use std::sync::Arc;

use ozzie_core::domain::{ConversationManager, Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

// ---- new_conversation ----

pub struct NewConversationTool {
    manager: Arc<dyn ConversationManager>,
}

impl NewConversationTool {
    pub fn new(manager: Arc<dyn ConversationManager>) -> Self {
        Self { manager }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "new_conversation".to_string(),
            description: "Start a fresh conversation thread and make it active. Use when the \
                topic shifts away from the current conversation. Returns the new conversation id."
                .to_string(),
            parameters: schema_for::<NewConversationInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct NewConversationInput {
    /// Optional human-readable title for the conversation.
    #[serde(default)]
    title: Option<String>,
}

#[async_trait::async_trait]
impl Tool for NewConversationTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "new_conversation",
            "Create a new conversation and switch to it",
            Self::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: NewConversationInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;
        let id = self
            .manager
            .create(input.title)
            .await
            .map_err(|e| ToolError::Execution(format!("create conversation: {e}")))?;
        Ok(serde_json::json!({ "conversation_id": id }).to_string())
    }
}

// ---- switch_conversation ----

pub struct SwitchConversationTool {
    manager: Arc<dyn ConversationManager>,
}

impl SwitchConversationTool {
    pub fn new(manager: Arc<dyn ConversationManager>) -> Self {
        Self { manager }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "switch_conversation".to_string(),
            description: "Change the active conversation. The switch takes effect at the next \
                user message; the current turn finishes in its origin conversation."
                .to_string(),
            parameters: schema_for::<SwitchConversationInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct SwitchConversationInput {
    /// Id of the conversation to make active.
    conversation_id: String,
}

#[async_trait::async_trait]
impl Tool for SwitchConversationTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "switch_conversation",
            "Switch the active conversation",
            Self::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: SwitchConversationInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;
        let previous = self.manager.set_active(&input.conversation_id);
        Ok(serde_json::json!({
            "conversation_id": input.conversation_id,
            "previous": previous,
        })
        .to_string())
    }
}

// ---- list_conversations ----

pub struct ListConversationsTool {
    manager: Arc<dyn ConversationManager>,
}

impl ListConversationsTool {
    pub fn new(manager: Arc<dyn ConversationManager>) -> Self {
        Self { manager }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "list_conversations".to_string(),
            description: "List known conversations (most recent first). Each entry carries \
                id, title, status (active|archived), message_count, updated_at, is_active."
                .to_string(),
            parameters: schema_for::<ListConversationsInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ListConversationsInput {
    /// Include archived conversations in the result (default: false).
    #[serde(default)]
    include_archived: bool,
}

#[async_trait::async_trait]
impl Tool for ListConversationsTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "list_conversations",
            "List conversations",
            Self::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: ListConversationsInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;
        let mut summaries = self
            .manager
            .list()
            .await
            .map_err(|e| ToolError::Execution(format!("list conversations: {e}")))?;
        if !input.include_archived {
            summaries.retain(|s| {
                matches!(
                    s.status,
                    ozzie_core::domain::ConversationStatus::Active
                )
            });
        }
        serde_json::to_string(&summaries)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}

// ---- close_conversation ----

pub struct CloseConversationTool {
    manager: Arc<dyn ConversationManager>,
}

impl CloseConversationTool {
    pub fn new(manager: Arc<dyn ConversationManager>) -> Self {
        Self { manager }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "close_conversation".to_string(),
            description: "Archive a conversation (freeze + hide, history preserved). Use when \
                a topic is resolved or no longer relevant. An incoming message can revive an \
                archived conversation later. Defaults to the currently active conversation."
                .to_string(),
            parameters: schema_for::<CloseConversationInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct CloseConversationInput {
    /// Id of the conversation to archive. Defaults to the currently active conversation.
    #[serde(default)]
    conversation_id: Option<String>,
}

#[async_trait::async_trait]
impl Tool for CloseConversationTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "close_conversation",
            "Archive a conversation",
            Self::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: CloseConversationInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;
        let id = match input.conversation_id {
            Some(id) => id,
            None => self.manager.active().ok_or_else(|| {
                ToolError::Execution("no active conversation to close".to_string())
            })?,
        };
        self.manager
            .archive(&id)
            .await
            .map_err(|e| ToolError::Execution(format!("archive conversation: {e}")))?;
        Ok(serde_json::json!({ "archived": id }).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ozzie_core::domain::{ConversationError, ConversationStatus, ConversationSummary};
    use std::sync::Mutex;

    struct MockManager {
        active: Mutex<Option<String>>,
        conversations: Mutex<Vec<ConversationSummary>>,
        next_id: Mutex<usize>,
    }

    impl MockManager {
        fn new() -> Self {
            Self {
                active: Mutex::new(None),
                conversations: Mutex::new(Vec::new()),
                next_id: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl ConversationManager for MockManager {
        fn active(&self) -> Option<String> {
            self.active.lock().unwrap().clone()
        }

        fn set_active(&self, id: &str) -> Option<String> {
            let mut guard = self.active.lock().unwrap();
            let previous = guard.clone();
            *guard = Some(id.to_string());
            previous
        }

        async fn create(&self, title: Option<String>) -> Result<String, ConversationError> {
            let mut counter = self.next_id.lock().unwrap();
            *counter += 1;
            let id = format!("conv_{}", *counter);
            self.conversations.lock().unwrap().push(ConversationSummary {
                id: id.clone(),
                title,
                status: ConversationStatus::Active,
                message_count: 0,
                updated_at: chrono::Utc::now(),
                is_active: true,
            });
            *self.active.lock().unwrap() = Some(id.clone());
            Ok(id)
        }

        async fn list(&self) -> Result<Vec<ConversationSummary>, ConversationError> {
            Ok(self.conversations.lock().unwrap().clone())
        }

        async fn archive(&self, id: &str) -> Result<(), ConversationError> {
            let mut convs = self.conversations.lock().unwrap();
            for c in convs.iter_mut() {
                if c.id == id {
                    c.status = ConversationStatus::Archived;
                    c.is_active = false;
                }
            }
            let mut active = self.active.lock().unwrap();
            if active.as_deref() == Some(id) {
                *active = None;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn new_conversation_creates_and_returns_id() {
        let mgr = Arc::new(MockManager::new());
        let tool = NewConversationTool::new(mgr.clone());

        let out = tool.run(r#"{"title":"test"}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["conversation_id"].as_str().unwrap().starts_with("conv_"));
        assert!(mgr.active().is_some());
    }

    #[tokio::test]
    async fn switch_conversation_changes_active() {
        let mgr = Arc::new(MockManager::new());
        let create = NewConversationTool::new(mgr.clone());
        create.run(r#"{}"#).await.unwrap();
        let first = mgr.active().unwrap();

        let switch = SwitchConversationTool::new(mgr.clone());
        let out = switch
            .run(r#"{"conversation_id":"conv_elsewhere"}"#)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["conversation_id"], "conv_elsewhere");
        assert_eq!(v["previous"].as_str(), Some(first.as_str()));
        assert_eq!(mgr.active().as_deref(), Some("conv_elsewhere"));
    }

    #[tokio::test]
    async fn list_conversations_filters_archived_by_default() {
        let mgr = Arc::new(MockManager::new());
        let create = NewConversationTool::new(mgr.clone());
        create.run(r#"{"title":"A"}"#).await.unwrap();
        let a = mgr.active().unwrap();
        create.run(r#"{"title":"B"}"#).await.unwrap();

        // archive A
        let close = CloseConversationTool::new(mgr.clone());
        close
            .run(&format!(r#"{{"conversation_id":"{a}"}}"#))
            .await
            .unwrap();

        let list = ListConversationsTool::new(mgr.clone());
        let out = list.run(r#"{}"#).await.unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(v.len(), 1, "archived conversations filtered by default");

        let out = list.run(r#"{"include_archived":true}"#).await.unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[tokio::test]
    async fn close_conversation_defaults_to_active() {
        let mgr = Arc::new(MockManager::new());
        let create = NewConversationTool::new(mgr.clone());
        create.run(r#"{}"#).await.unwrap();
        let id = mgr.active().unwrap();

        let close = CloseConversationTool::new(mgr.clone());
        let out = close.run(r#"{}"#).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["archived"], id);
        assert!(mgr.active().is_none());
    }

    #[tokio::test]
    async fn close_conversation_errors_without_active() {
        let mgr = Arc::new(MockManager::new());
        let close = CloseConversationTool::new(mgr);
        let err = close.run(r#"{}"#).await.unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }
}
