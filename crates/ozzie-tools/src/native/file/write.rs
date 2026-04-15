use ozzie_core::domain::{Tool, ToolError, ToolInfo, TOOL_CTX};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::debug;

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

        // Auto-commit if configured for the current workspace
        let auto_commit = TOOL_CTX
            .try_with(|ctx| ctx.git_auto_commit && ctx.work_dir.is_some())
            .unwrap_or(false);

        if auto_commit
            && let Err(e) = git_auto_commit(&path)
        {
            debug!(error = %e, path = %path, "auto-commit failed (non-fatal)");
        }

        let result = FileWriteResult {
            path,
            bytes_written: bytes.len(),
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

/// Stages and commits a single file with a descriptive message.
fn git_auto_commit(file_path: &str) -> Result<(), String> {
    let path = std::path::Path::new(file_path);

    // Find the git repo root by walking up
    let repo_dir = path
        .parent()
        .ok_or_else(|| "no parent directory".to_string())?;

    let add = std::process::Command::new("git")
        .args(["add", file_path])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| format!("git add: {e}"))?;

    if !add.status.success() {
        return Err(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        ));
    }

    // Check if there's anything to commit
    let status = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(repo_dir)
        .status()
        .map_err(|e| format!("git diff: {e}"))?;

    if status.success() {
        // Nothing staged — file unchanged
        return Ok(());
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let commit = std::process::Command::new("git")
        .args(["commit", "-m", &format!("auto: update {file_name}")])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| format!("git commit: {e}"))?;

    if !commit.status.success() {
        return Err(format!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        ));
    }

    debug!(file = %file_path, "auto-committed");
    Ok(())
}
