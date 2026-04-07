use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

use super::SKIP_DIRS;

/// Searches for files matching a glob pattern.
pub struct GlobTool;

#[derive(Deserialize, JsonSchema)]
struct GlobArgs {
    /// Base directory to search in.
    path: String,
    /// Glob pattern (e.g. '**/*.rs', '*.go').
    pattern: String,
}

impl GlobTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "glob".to_string(),
            description: "Search for files matching a glob pattern".to_string(),
            parameters: schema_for::<GlobArgs>(),
            dangerous: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for GlobTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "glob",
            "Search for files matching a glob pattern",
            GlobTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: GlobArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let resolved = super::resolve_path(&args.path);
        super::enforce_path_jail(&resolved)?;
        let base = std::path::PathBuf::from(&resolved);
        if !base.is_dir() {
            return Err(ToolError::Execution(format!(
                "'{}' is not a directory",
                resolved
            )));
        }

        let full_pattern = base.join(&args.pattern).to_string_lossy().to_string();
        let matches: Vec<String> = glob::glob(&full_pattern)
            .map_err(|e| ToolError::Execution(format!("invalid glob pattern: {e}")))?
            .filter_map(|entry| entry.ok())
            .filter(|p| {
                !p.components().any(|c| {
                    let s = c.as_os_str().to_string_lossy();
                    SKIP_DIRS.contains(&s.as_ref())
                })
            })
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        #[derive(Serialize)]
        struct GlobResult {
            matches: Vec<String>,
            count: usize,
        }

        let result = GlobResult {
            count: matches.len(),
            matches,
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn glob_matches_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(dir.path().join("hello.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("world.txt"), "text").unwrap();

        let tool = GlobTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "*.rs"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("hello.rs"));
        assert!(!result.contains("world.txt"));
    }
}
