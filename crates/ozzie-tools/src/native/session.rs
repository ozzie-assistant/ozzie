use std::collections::HashMap;
use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo, TOOL_CTX};
use ozzie_core::domain::ConversationStore;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Updates the current session metadata (title, language, working directory, etc.).
pub struct UpdateSessionTool {
    store: Arc<dyn ConversationStore>,
}

impl UpdateSessionTool {
    pub fn new(store: Arc<dyn ConversationStore>) -> Self {
        Self { store }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "update_session".to_string(),
            description: "Update the current session metadata. Only provided fields are updated; omitted fields are left unchanged.".to_string(),
            parameters: schema_for::<UpdateSessionInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for the update_session tool.
#[derive(Deserialize, JsonSchema)]
struct UpdateSessionInput {
    /// Working directory path for the session.
    #[serde(default)]
    root_dir: Option<String>,
    /// Preferred response language (e.g. "fr", "en").
    #[serde(default)]
    language: Option<String>,
    /// Conversation title.
    #[serde(default)]
    title: Option<String>,
    /// Arbitrary key-value metadata to merge into the session.
    #[serde(default)]
    metadata: Option<HashMap<String, String>>,
}

#[async_trait::async_trait]
impl Tool for UpdateSessionTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "update_session",
            "Update session metadata",
            UpdateSessionTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: UpdateSessionInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        // Get session ID from task-local context
        let conversation_id = TOOL_CTX
            .try_with(|ctx| ctx.conversation_id.clone())
            .unwrap_or_default();

        if conversation_id.is_empty() {
            return Err(ToolError::Execution(
                "no session in context".to_string(),
            ));
        }

        let mut session = self
            .store
            .get(&conversation_id)
            .await
            .map_err(|e| ToolError::Execution(format!("get session: {e}")))?
            .ok_or_else(|| ToolError::Execution(format!("session not found: {conversation_id}")))?;

        // Update only provided fields
        if let Some(root_dir) = input.root_dir {
            session.root_dir = Some(root_dir);
        }
        if let Some(language) = input.language {
            session.language = Some(language);
        }
        if let Some(title) = input.title {
            session.title = Some(title);
        }

        // Merge metadata
        if let Some(meta) = input.metadata {
            for (k, v) in meta {
                session.metadata.insert(k, v);
            }
        }

        session.updated_at = chrono::Utc::now();

        self.store
            .update(&session)
            .await
            .map_err(|e| ToolError::Execution(format!("update session: {e}")))?;

        serde_json::to_string(&session)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}
