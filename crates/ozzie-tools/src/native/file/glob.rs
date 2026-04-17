use std::path::{Path, PathBuf};

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

use super::sanitize_glob;

/// Searches for files matching a glob pattern, with gitignore support.
pub struct GlobTool;

#[derive(Deserialize, JsonSchema)]
struct GlobArgs {
    /// Base directory to search in.
    path: String,
    /// Glob pattern (e.g. '**/*.rs', '*.go', '*.{ts,tsx}').
    pattern: String,
    /// Maximum number of results (default 200).
    #[serde(default)]
    max_results: Option<usize>,
    /// Sort by modification time (newest first). Default false.
    #[serde(default)]
    sort_by_mtime: Option<bool>,
}

const DEFAULT_MAX_RESULTS: usize = 200;

impl GlobTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "glob".to_string(),
            description: "Search for files matching a glob pattern (gitignore-aware)".to_string(),
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
            "Search for files matching a glob pattern (gitignore-aware)",
            GlobTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: GlobArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let resolved = super::resolve_path(&args.path);
        super::enforce_path_jail(&resolved)?;
        let base = PathBuf::from(&resolved);
        if !base.is_dir() {
            return Err(ToolError::Execution(format!(
                "'{}' is not a directory",
                resolved
            )));
        }

        let max_results = args.max_results.unwrap_or(DEFAULT_MAX_RESULTS);
        let sort_mtime = args.sort_by_mtime.unwrap_or(false);

        let matches = glob_walk(&base, &args.pattern, max_results, sort_mtime)
            .map_err(|e| ToolError::Execution(format!("glob: {e}")))?;

        #[derive(Serialize)]
        struct GlobResult {
            matches: Vec<String>,
            count: usize,
            truncated: bool,
        }

        let truncated = matches.len() >= max_results;
        let result = GlobResult {
            count: matches.len(),
            matches,
            truncated,
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

/// Walk directory using `ignore` (gitignore-aware) and match files against a globset pattern.
fn glob_walk(
    root: &Path,
    pattern: &str,
    max_results: usize,
    sort_mtime: bool,
) -> Result<Vec<String>, String> {
    let sanitized = sanitize_glob(pattern);

    let glob = globset::GlobBuilder::new(&sanitized)
        .literal_separator(false)
        .build()
        .map_err(|e| format!("invalid glob pattern '{sanitized}': {e}"))?
        .compile_matcher();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .build();

    let mut results: Vec<(String, Option<std::time::SystemTime>)> = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let Some(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_file() {
            continue;
        }

        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path);

        if !glob.is_match(relative) {
            continue;
        }

        let mtime = if sort_mtime {
            entry.metadata().ok().and_then(|m| m.modified().ok())
        } else {
            None
        };

        results.push((path.to_string_lossy().to_string(), mtime));

        // Over-collect slightly for sorting, but cap at 4x to avoid scanning everything
        if !sort_mtime && results.len() >= max_results {
            break;
        }
        if sort_mtime && results.len() >= max_results * 4 {
            break;
        }
    }

    if sort_mtime {
        results.sort_by_key(|r| std::cmp::Reverse(r.1));
    }

    let matches: Vec<String> = results
        .into_iter()
        .take(max_results)
        .map(|(p, _)| p)
        .collect();

    Ok(matches)
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

    #[tokio::test]
    async fn glob_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("top.rs"), "").unwrap();
        std::fs::write(dir.path().join("sub/nested.rs"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "**/*.rs"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("top.rs"));
        assert!(result.contains("nested.rs"));
    }

    #[tokio::test]
    async fn glob_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored/secret.rs"), "").unwrap();
        std::fs::write(dir.path().join("visible.rs"), "").unwrap();

        let tool = GlobTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "**/*.rs"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("visible.rs"));
        assert!(!result.contains("secret.rs"));
    }

    #[tokio::test]
    async fn glob_max_results() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file{i}.txt")), "").unwrap();
        }

        let tool = GlobTool;
        let result = tool
            .run(&serde_json::json!({
                "path": dir_str, "pattern": "*.txt", "max_results": 3
            }).to_string())
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"].as_u64().unwrap(), 3);
        assert!(parsed["truncated"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn glob_sort_by_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(dir.path().join("old.txt"), "old").unwrap();
        // Small delay to ensure different mtime
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(dir.path().join("new.txt"), "new").unwrap();

        let tool = GlobTool;
        let result = tool
            .run(&serde_json::json!({
                "path": dir_str, "pattern": "*.txt", "sort_by_mtime": true
            }).to_string())
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let matches = parsed["matches"].as_array().unwrap();
        let first = matches[0].as_str().unwrap();
        assert!(first.contains("new.txt"), "newest file should be first, got {first}");
    }
}
