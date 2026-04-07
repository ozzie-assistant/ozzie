use std::collections::HashMap;
use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_memory::{EmbedJob, Pipeline, Store};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Deletes a memory entry by ID.
pub struct ForgetMemoryTool {
    store: Arc<dyn Store>,
    pipeline: Option<Arc<Pipeline>>,
}

impl ForgetMemoryTool {
    pub fn new(store: Arc<dyn Store>, pipeline: Option<Arc<Pipeline>>) -> Self {
        Self { store, pipeline }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "forget_memory".to_string(),
            description: "Delete a specific memory entry by its ID. Use this when information is no longer relevant or was stored incorrectly.".to_string(),
            parameters: schema_for::<ForgetMemoryArgs>(),
            dangerous: false,
        }
    }
}

/// Arguments for forget_memory.
#[derive(Deserialize, JsonSchema)]
struct ForgetMemoryArgs {
    /// The memory ID to delete (e.g., mem_abc12345).
    id: String,
}

#[async_trait::async_trait]
impl Tool for ForgetMemoryTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "forget_memory",
            "Delete a memory entry by ID",
            ForgetMemoryTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: ForgetMemoryArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("forget_memory: parse input: {e}")))?;

        self.store
            .delete(&args.id)
            .await
            .map_err(|e| ToolError::Execution(format!("forget_memory: {e}")))?;

        if let Some(pipeline) = &self.pipeline {
            pipeline.enqueue(EmbedJob {
                id: args.id.clone(),
                content: String::new(),
                meta: HashMap::new(),
                delete: true,
            });
        }

        let result = serde_json::json!({
            "id": args.id,
            "status": "deleted",
        });
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::store::StoreMemoryTool;
    use super::super::testutil::FakeStore;

    #[tokio::test]
    async fn forget_memory_basic() {
        let store = FakeStore::new_arc();

        let store_tool = StoreMemoryTool::new(store.clone(), None);
        let result = store_tool
            .run(r#"{"type":"fact","title":"Test","content":"test content"}"#)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let id = parsed["id"].as_str().unwrap().to_string();

        let forget_tool = ForgetMemoryTool::new(store.clone(), None);
        let result = forget_tool
            .run(&format!(r#"{{"id":"{id}"}}"#))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "deleted");

        let entries = store.list().await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn forget_memory_missing_id() {
        let store = FakeStore::new_arc();
        let tool = ForgetMemoryTool::new(store, None);

        let result = tool.run(r#"{}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn forget_memory_not_found() {
        let store = FakeStore::new_arc();
        let tool = ForgetMemoryTool::new(store, None);

        let result = tool.run(r#"{"id":"mem_nonexistent"}"#).await;
        assert!(result.is_err());
    }
}
