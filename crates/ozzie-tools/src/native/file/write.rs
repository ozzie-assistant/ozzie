use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Writes content to a file on disk.
pub struct FileWriteTool;

#[derive(Deserialize, JsonSchema)]
struct FileWriteArgs {
    /// File path to write.
    path: String,
    /// Content to write.
    content: String,
    /// Append to file instead of overwriting.
    #[serde(default)]
    append: bool,
}

#[derive(Serialize)]
struct FileWriteResult {
    path: String,
    bytes_written: usize,
}

impl FileWriteTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "file_write".to_string(),
            description: "Write content to a file".to_string(),
            parameters: schema_for::<FileWriteArgs>(),
            dangerous: true,
        }
    }
}

#[async_trait::async_trait]
impl Tool for FileWriteTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "file_write",
            "Write content to a file",
            FileWriteTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: FileWriteArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let path = super::resolve_path(&args.path);
        super::enforce_path_jail(&path)?;

        if let Some(parent) = std::path::Path::new(&path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("create dir: {e}")))?;
        }

        let bytes = args.content.as_bytes();

        if args.append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|e| ToolError::Execution(format!("open file '{}': {e}", path)))?;
            file.write_all(bytes)
                .await
                .map_err(|e| ToolError::Execution(format!("write file: {e}")))?;
        } else {
            tokio::fs::write(&path, bytes)
                .await
                .map_err(|e| ToolError::Execution(format!("write file '{}': {e}", path)))?;
        }

        let result = FileWriteResult {
            path,
            bytes_written: bytes.len(),
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}
