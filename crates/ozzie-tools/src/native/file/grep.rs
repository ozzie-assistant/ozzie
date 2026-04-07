use std::io::BufRead;
use std::path::Path;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

use super::SKIP_DIRS;

/// Searches for text patterns in files.
pub struct GrepTool;

#[derive(Deserialize, JsonSchema)]
struct GrepArgs {
    /// Directory or file to search in.
    path: String,
    /// Text pattern to search for (literal string).
    pattern: String,
    /// Optional filename glob filter (e.g. '*.rs').
    #[serde(default)]
    glob_filter: Option<String>,
}

const MAX_GREP_MATCHES: usize = 100;

impl GrepTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "grep".to_string(),
            description: "Search for text patterns in files".to_string(),
            parameters: schema_for::<GrepArgs>(),
            dangerous: false,
        }
    }
}

#[derive(Serialize)]
struct GrepMatch {
    path: String,
    line: usize,
    content: String,
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "grep",
            "Search for text patterns in files",
            GrepTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: GrepArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let resolved = super::resolve_path(&args.path);
        super::enforce_path_jail(&resolved)?;
        let search_path = std::path::PathBuf::from(resolved);
        let mut matches = Vec::new();

        grep_walk(&search_path, &args.pattern, &args.glob_filter, &mut matches)
            .map_err(|e| ToolError::Execution(format!("grep: {e}")))?;

        #[derive(Serialize)]
        struct GrepResult {
            matches: Vec<GrepMatch>,
            count: usize,
            truncated: bool,
        }

        let truncated = matches.len() >= MAX_GREP_MATCHES;
        let result = GrepResult {
            count: matches.len(),
            matches,
            truncated,
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

fn grep_walk(
    path: &Path,
    pattern: &str,
    glob_filter: &Option<String>,
    matches: &mut Vec<GrepMatch>,
) -> std::io::Result<()> {
    if matches.len() >= MAX_GREP_MATCHES {
        return Ok(());
    }

    let meta = std::fs::metadata(path)?;
    if meta.is_file() {
        grep_file(path, pattern, matches)?;
        return Ok(());
    }

    if !meta.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();

        if entry.file_type()?.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            grep_walk(&entry.path(), pattern, glob_filter, matches)?;
        } else if entry.file_type()?.is_file() {
            if let Some(filter) = glob_filter {
                let matched = glob::Pattern::new(filter).is_ok_and(|p| p.matches(&name));
                if !matched {
                    continue;
                }
            }
            if is_binary(&entry.path()) {
                continue;
            }
            grep_file(&entry.path(), pattern, matches)?;
        }

        if matches.len() >= MAX_GREP_MATCHES {
            break;
        }
    }

    Ok(())
}

fn grep_file(path: &Path, pattern: &str, matches: &mut Vec<GrepMatch>) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if line.contains(pattern) {
            matches.push(GrepMatch {
                path: path.to_string_lossy().to_string(),
                line: i + 1,
                content: line,
            });
            if matches.len() >= MAX_GREP_MATCHES {
                break;
            }
        }
    }
    Ok(())
}

/// Check if a file appears binary by looking for null bytes in the first 512 bytes.
fn is_binary(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    let buf = reader.fill_buf().unwrap_or(&[]);
    let check_len = buf.len().min(512);
    buf[..check_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn grep_finds_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(
            dir.path().join("file.txt"),
            "hello world\nfoo bar\nhello again",
        )
        .unwrap();

        let tool = GrepTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "hello"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("hello world"));
        assert!(result.contains("hello again"));
        assert!(!result.contains("foo bar"));
    }

    #[tokio::test]
    async fn grep_with_glob_filter() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(dir.path().join("code.rs"), "fn hello() {}").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello notes").unwrap();

        let tool = GrepTool;
        let result = tool
            .run(&serde_json::json!({
                "path": dir_str, "pattern": "hello", "glob_filter": "*.rs"
            }).to_string())
            .await
            .unwrap();

        assert!(result.contains("code.rs"));
        assert!(!result.contains("notes.txt"));
    }
}
