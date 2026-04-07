use std::path::Path;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

use super::SKIP_DIRS;

/// Lists directory contents with depth control.
pub struct ListDirTool;

#[derive(Deserialize, JsonSchema)]
struct ListDirArgs {
    /// Directory path to list.
    path: String,
    /// Maximum traversal depth (default: 2).
    #[serde(default = "default_depth")]
    depth: usize,
}

fn default_depth() -> usize {
    2
}

impl ListDirTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "list_dir".to_string(),
            description: "List directory contents with depth control".to_string(),
            parameters: schema_for::<ListDirArgs>(),
            dangerous: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListDirTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "list_dir",
            "List directory contents",
            ListDirTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: ListDirArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let resolved = super::resolve_path(&args.path);
        super::enforce_path_jail(&resolved)?;
        let root = std::path::PathBuf::from(&resolved);
        if !root.is_dir() {
            return Err(ToolError::Execution(format!(
                "'{}' is not a directory",
                resolved
            )));
        }

        let mut entries = Vec::new();
        collect_entries(&root, &root, args.depth, &mut entries)
            .map_err(|e| ToolError::Execution(format!("list dir: {e}")))?;

        entries.sort();
        Ok(entries.join("\n"))
    }
}

fn collect_entries(
    root: &Path,
    current: &Path,
    max_depth: usize,
    entries: &mut Vec<String>,
) -> std::io::Result<()> {
    let rel_depth = current
        .strip_prefix(root)
        .unwrap_or(current)
        .components()
        .count();

    if rel_depth >= max_depth {
        return Ok(());
    }

    let read_dir = std::fs::read_dir(current)?;
    for entry in read_dir {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let is_dir = entry.file_type()?.is_dir();
        if is_dir {
            entries.push(format!("{rel}/"));
            collect_entries(root, &path, max_depth, entries)?;
        } else {
            entries.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_dir_basic() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(dir.path().join("a.txt"), "content").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(dir.path().join("subdir").join("b.txt"), "content").unwrap();

        let tool = ListDirTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str}).to_string())
            .await
            .unwrap();

        assert!(result.contains("a.txt"));
        assert!(result.contains("subdir/"));
    }
}
