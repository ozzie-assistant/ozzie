use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_memory::Store;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Searches through stored memories by keyword query and optional tags.
pub struct QueryMemoriesTool {
    store: Arc<dyn Store>,
}

impl QueryMemoriesTool {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "query_memories".to_string(),
            description: "Search through stored memories by keyword query and optional tags. Returns the most relevant results.".to_string(),
            parameters: schema_for::<QueryMemoriesArgs>(),
            dangerous: false,
        }
    }
}

/// Arguments for query_memories.
#[derive(Deserialize, JsonSchema)]
struct QueryMemoriesArgs {
    /// Search query keywords.
    query: String,
    /// Tags to filter by (e.g. "deploy,ci-cd" or ["deploy", "ci-cd"]).
    #[serde(default, deserialize_with = "super::store::deserialize_tags")]
    tags: Option<String>,
    /// Maximum number of results (default: 5).
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
struct QueryMemoryResult {
    id: String,
    title: String,
    #[serde(rename = "type")]
    memory_type: String,
    content: String,
    score: f64,
}

#[async_trait::async_trait]
impl Tool for QueryMemoriesTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "query_memories",
            "Search stored memories by query and tags",
            QueryMemoriesTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: QueryMemoriesArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("query_memories: parse input: {e}")))?;

        let tags: Vec<String> = args
            .tags
            .map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let limit = args.limit.unwrap_or(5);

        // Use keyword retriever backed by the shared store.
        let retriever = ozzie_memory::KeywordRetriever::new(StoreRef(self.store.clone()));
        let memories = retriever
            .retrieve(&args.query, &tags, limit)
            .await
            .map_err(|e| ToolError::Execution(format!("query_memories: {e}")))?;

        let results: Vec<QueryMemoryResult> = memories
            .into_iter()
            .map(|m| QueryMemoryResult {
                id: m.entry.id,
                title: m.entry.title,
                memory_type: format!("{:?}", m.entry.memory_type).to_lowercase(),
                content: m.content,
                score: m.score,
            })
            .collect();

        serde_json::to_string(&results)
            .map_err(|e| ToolError::Execution(format!("query_memories: marshal: {e}")))
    }
}

/// Wrapper to implement `Store` for `Arc<dyn Store>`.
struct StoreRef(Arc<dyn Store>);

#[async_trait::async_trait]
impl Store for StoreRef {
    async fn create(
        &self,
        entry: &mut ozzie_memory::MemoryEntry,
        content: &str,
    ) -> Result<(), ozzie_memory::MemoryError> {
        self.0.create(entry, content).await
    }

    async fn get(&self, id: &str) -> Result<(ozzie_memory::MemoryEntry, String), ozzie_memory::MemoryError> {
        self.0.get(id).await
    }

    async fn update(
        &self,
        entry: &ozzie_memory::MemoryEntry,
        content: &str,
    ) -> Result<(), ozzie_memory::MemoryError> {
        self.0.update(entry, content).await
    }

    async fn delete(&self, id: &str) -> Result<(), ozzie_memory::MemoryError> {
        self.0.delete(id).await
    }

    async fn list(&self) -> Result<Vec<ozzie_memory::MemoryEntry>, ozzie_memory::MemoryError> {
        self.0.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::FakeStore;

    #[tokio::test]
    async fn query_memories_empty() {
        let store = FakeStore::new_arc();
        let tool = QueryMemoriesTool::new(store);

        let result = tool
            .run(r#"{"query": "deploy convention"}"#)
            .await
            .unwrap();

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_empty());
    }

    #[tokio::test]
    async fn query_memories_with_data() {
        let store = FakeStore::new_arc();

        // Store a memory first
        let store_tool =
            super::super::StoreMemoryTool::new(store.clone(), None);
        store_tool
            .run(r#"{"title": "Deploy convention", "content": "Always use semver tags and CI/CD.", "tags": "deploy,ci-cd"}"#)
            .await
            .unwrap();

        let tool = QueryMemoriesTool::new(store);

        let result = tool
            .run(r#"{"query": "deploy", "limit": 5}"#)
            .await
            .unwrap();

        let parsed: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0]["title"].as_str().unwrap().contains("Deploy"));
    }
}
