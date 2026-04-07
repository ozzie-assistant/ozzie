use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Reads file contents from disk.
pub struct FileReadTool;

#[derive(Deserialize, JsonSchema)]
struct FileReadArgs {
    /// File path to read.
    path: String,
    /// Line offset to start from (0-indexed).
    #[serde(default)]
    offset: Option<usize>,
    /// Maximum number of lines.
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct FileReadResult {
    path: String,
    content: String,
    lines: usize,
}

impl FileReadTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "file_read".to_string(),
            description: "Read file contents from disk".to_string(),
            parameters: schema_for::<FileReadArgs>(),
            dangerous: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for FileReadTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "file_read",
            "Read file contents",
            FileReadTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: FileReadArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let path = super::resolve_path(&args.path);
        super::enforce_path_jail(&path)?;

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Execution(format!("read file '{}': {e}", path)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let offset = args.offset.unwrap_or(0).min(total);
        let limit = args.limit.unwrap_or(total - offset);
        let selected: Vec<&str> = lines.into_iter().skip(offset).take(limit).collect();

        let result = FileReadResult {
            path,
            content: selected.join("\n"),
            lines: selected.len(),
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let write_tool = super::super::FileWriteTool;
        write_tool
            .run(&serde_json::json!({"path": path_str, "content": "hello\nworld"}).to_string())
            .await
            .unwrap();

        let read_tool = FileReadTool;
        let result = read_tool
            .run(&serde_json::json!({"path": path_str}).to_string())
            .await
            .unwrap();

        let parsed: FileReadResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.content, "hello\nworld");
        assert_eq!(parsed.lines, 2);
    }

    #[tokio::test]
    async fn read_with_offset_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lines.txt");
        let path_str = path.to_str().unwrap();

        let write_tool = super::super::FileWriteTool;
        write_tool
            .run(&serde_json::json!({
                "path": path_str, "content": "line0\nline1\nline2\nline3"
            }).to_string())
            .await
            .unwrap();

        let read_tool = FileReadTool;
        let result = read_tool
            .run(&serde_json::json!({"path": path_str, "offset": 1, "limit": 2}).to_string())
            .await
            .unwrap();

        let parsed: FileReadResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.content, "line1\nline2");
        assert_eq!(parsed.lines, 2);
    }
}
