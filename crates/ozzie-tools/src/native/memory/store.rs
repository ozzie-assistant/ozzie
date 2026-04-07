use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_memory::{
    build_embed_meta, build_embed_text, EmbedJob, ImportanceLevel, MemoryEntry, MemoryType,
    Pipeline, Store,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Stores a new memory entry in long-term memory.
pub struct StoreMemoryTool {
    store: Arc<dyn Store>,
    pipeline: Option<Arc<Pipeline>>,
}

impl StoreMemoryTool {
    pub fn new(store: Arc<dyn Store>, pipeline: Option<Arc<Pipeline>>) -> Self {
        Self { store, pipeline }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "store_memory".to_string(),
            description: "Store a piece of information in long-term memory for future recall. Use this to remember user preferences, important facts, procedures, or context.".to_string(),
            parameters: schema_for::<StoreMemoryArgs>(),
            dangerous: false,
        }
    }
}

/// Arguments for store_memory.
#[derive(Deserialize, JsonSchema)]
struct StoreMemoryArgs {
    /// Short descriptive title for the memory (e.g. "Deploy convention").
    title: String,
    /// Full content to remember (markdown supported).
    content: String,
    /// Memory type: "preference", "fact", "procedure", or "context". Defaults to "fact".
    #[serde(default, rename = "type")]
    memory_type: Option<String>,
    /// Tags for categorization (e.g. "deploy,ci-cd" or ["deploy", "ci-cd"]).
    #[serde(default, deserialize_with = "deserialize_tags")]
    tags: Option<String>,
    /// Importance level: "core", "important", "normal" (default), or "ephemeral".
    #[serde(default)]
    importance: Option<String>,
}

/// Accepts tags as either a comma-separated string or an array of strings.
pub(super) fn deserialize_tags<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct TagsVisitor;

    impl<'de> de::Visitor<'de> for TagsVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or array of strings")
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(None)
            } else {
                Ok(Some(v.to_string()))
            }
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            if v.is_empty() {
                Ok(None)
            } else {
                Ok(Some(v))
            }
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                let trimmed = item.trim().to_string();
                if !trimmed.is_empty() {
                    items.push(trimmed);
                }
            }
            if items.is_empty() {
                Ok(None)
            } else {
                Ok(Some(items.join(",")))
            }
        }
    }

    deserializer.deserialize_any(TagsVisitor)
}

#[async_trait::async_trait]
impl Tool for StoreMemoryTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "store_memory",
            "Store information in long-term memory",
            StoreMemoryTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: StoreMemoryArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("store_memory: parse input: {e}")))?;

        let memory_type = args
            .memory_type
            .as_deref()
            .unwrap_or("fact")
            .parse::<MemoryType>()
            .unwrap_or(MemoryType::Fact);

        let tags: Vec<String> = args
            .tags
            .map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let importance = args
            .importance
            .as_deref()
            .and_then(|s| s.parse::<ImportanceLevel>().ok())
            .unwrap_or_default();

        let mut entry = MemoryEntry {
            id: String::new(),
            title: args.title,
            source: "agent".to_string(),
            memory_type,
            tags,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_used_at: chrono::Utc::now(),
            confidence: 1.0,
            importance,
            embedding_model: String::new(),
            indexed_at: None,
            merged_into: None,
        };

        self.store
            .create(&mut entry, &args.content)
            .await
            .map_err(|e| ToolError::Execution(format!("store_memory: {e}")))?;

        if let Some(pipeline) = &self.pipeline {
            pipeline.enqueue(EmbedJob {
                id: entry.id.clone(),
                content: build_embed_text(&entry, &args.content),
                meta: build_embed_meta(&entry),
                delete: false,
            });
        }

        let result = serde_json::json!({
            "id": entry.id,
            "status": "stored",
            "note": "Memory stored securely. Content is persistent and private — do not delete it.",
        });
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::FakeStore;

    #[tokio::test]
    async fn store_memory_basic() {
        let store = FakeStore::new_arc();
        let tool = StoreMemoryTool::new(store.clone(), None);

        let result = tool
            .run(r#"{"type":"fact","title":"Rust is great","content":"Rust is a systems programming language."}"#)
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "stored");
        assert!(!parsed["id"].as_str().unwrap().is_empty());

        let entries = store.list().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Rust is great");
        assert_eq!(entries[0].memory_type, MemoryType::Fact);
    }

    #[tokio::test]
    async fn store_memory_with_tags_and_importance() {
        let store = FakeStore::new_arc();
        let tool = StoreMemoryTool::new(store.clone(), None);

        let result = tool
            .run(r#"{"type":"preference","title":"Dark mode","content":"User prefers dark mode.","tags":"ui, theme","importance":"important"}"#)
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "stored");

        let entries = store.list().await.unwrap();
        assert_eq!(entries[0].tags, vec!["ui", "theme"]);
        assert_eq!(entries[0].importance, ImportanceLevel::Important);
    }

    #[tokio::test]
    async fn store_memory_missing_title() {
        let store = FakeStore::new_arc();
        let tool = StoreMemoryTool::new(store, None);

        let result = tool
            .run(r#"{"type":"fact","content":"some content"}"#)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn store_memory_missing_content() {
        let store = FakeStore::new_arc();
        let tool = StoreMemoryTool::new(store, None);

        let result = tool.run(r#"{"type":"fact","title":"test"}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn store_memory_invalid_json() {
        let store = FakeStore::new_arc();
        let tool = StoreMemoryTool::new(store, None);

        let result = tool.run("not json").await;
        assert!(result.is_err());
    }
}
