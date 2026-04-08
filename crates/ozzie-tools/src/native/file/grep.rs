use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Searches for text patterns in files using regex, with gitignore support
/// and parallel directory walking.
pub struct GrepTool;

#[derive(Deserialize, JsonSchema)]
struct GrepArgs {
    /// Directory or file to search in.
    path: String,
    /// Pattern to search for (regex syntax).
    pattern: String,
    /// Optional filename glob filter (e.g. '*.rs').
    #[serde(default)]
    glob_filter: Option<String>,
    /// Number of context lines before each match (default 0).
    #[serde(default)]
    context_before: Option<usize>,
    /// Number of context lines after each match (default 0).
    #[serde(default)]
    context_after: Option<usize>,
    /// Maximum number of matches to return (default 100).
    #[serde(default)]
    max_matches: Option<usize>,
}

const DEFAULT_MAX_MATCHES: usize = 100;

impl GrepTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "grep".to_string(),
            description: "Search for text patterns in files (regex, gitignore-aware)".to_string(),
            parameters: schema_for::<GrepArgs>(),
            dangerous: false,
        }
    }
}

#[derive(Serialize, Clone)]
struct GrepMatch {
    path: String,
    line: usize,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context_after: Vec<String>,
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "grep",
            "Search for text patterns in files (regex, gitignore-aware)",
            GrepTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: GrepArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let resolved = super::resolve_path(&args.path);
        super::enforce_path_jail(&resolved)?;

        let search_path = PathBuf::from(&resolved);
        let max_matches = args.max_matches.unwrap_or(DEFAULT_MAX_MATCHES);
        let ctx_before = args.context_before.unwrap_or(0);
        let ctx_after = args.context_after.unwrap_or(0);

        let sanitized = sanitize_pattern(&args.pattern);
        let re = regex::Regex::new(&sanitized)
            .map_err(|e| ToolError::Execution(format!("invalid regex pattern: {e}")))?;

        let matches = if search_path.is_file() {
            let mut results = Vec::new();
            grep_file(&search_path, &re, ctx_before, ctx_after, max_matches, &mut results)
                .map_err(|e| ToolError::Execution(format!("grep: {e}")))?;
            results
        } else {
            grep_directory(&search_path, &re, &args.glob_filter, ctx_before, ctx_after, max_matches)
                .map_err(|e| ToolError::Execution(format!("grep: {e}")))?
        };

        #[derive(Serialize)]
        struct GrepResult {
            matches: Vec<GrepMatch>,
            count: usize,
            truncated: bool,
        }

        let truncated = matches.len() >= max_matches;
        let result = GrepResult {
            count: matches.len(),
            matches,
            truncated,
        };

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

/// Walk a directory using `ignore` (parallel, gitignore-aware) and collect grep matches.
fn grep_directory(
    root: &Path,
    re: &regex::Regex,
    glob_filter: &Option<String>,
    ctx_before: usize,
    ctx_after: usize,
    max_matches: usize,
) -> Result<Vec<GrepMatch>, String> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(true).git_ignore(true).git_global(true);

    if let Some(filter) = glob_filter {
        let sanitized = super::sanitize_glob(filter);
        let mut types = ignore::types::TypesBuilder::new();
        types
            .add("filter", &sanitized)
            .map_err(|e| format!("invalid glob filter: {e}"))?;
        types.select("filter");
        builder.types(types.build().map_err(|e| format!("build types: {e}"))?);
    }

    let collected = Arc::new(Mutex::new(Vec::<GrepMatch>::new()));
    let count = Arc::new(AtomicUsize::new(0));

    let walker = builder.build_parallel();
    let re_clone = re.clone();

    walker.run(|| {
        let re = re_clone.clone();
        let collected = Arc::clone(&collected);
        let count = Arc::clone(&count);
        Box::new(move |entry| {
            if count.load(Ordering::Relaxed) >= max_matches {
                return ignore::WalkState::Quit;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let Some(ft) = entry.file_type() else {
                return ignore::WalkState::Continue;
            };
            if !ft.is_file() {
                return ignore::WalkState::Continue;
            }

            if is_binary(entry.path()) {
                return ignore::WalkState::Continue;
            }

            let mut file_matches = Vec::new();
            let remaining = max_matches.saturating_sub(count.load(Ordering::Relaxed));
            if remaining == 0 {
                return ignore::WalkState::Quit;
            }

            if grep_file(entry.path(), &re, ctx_before, ctx_after, remaining, &mut file_matches).is_ok()
                && !file_matches.is_empty()
            {
                let found = file_matches.len();
                if let Ok(mut all) = collected.lock() {
                    all.extend(file_matches);
                }
                count.fetch_add(found, Ordering::Relaxed);
            }

            if count.load(Ordering::Relaxed) >= max_matches {
                ignore::WalkState::Quit
            } else {
                ignore::WalkState::Continue
            }
        })
    });

    let mut results = match Arc::try_unwrap(collected) {
        Ok(mutex) => mutex.into_inner().unwrap_or_default(),
        Err(arc) => arc.lock().unwrap().clone(),
    };

    results.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    results.truncate(max_matches);
    Ok(results)
}

/// Grep a single file, collecting matches with optional context lines.
fn grep_file(
    path: &Path,
    re: &regex::Regex,
    ctx_before: usize,
    ctx_after: usize,
    max_matches: usize,
    matches: &mut Vec<GrepMatch>,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let path_str = path.to_string_lossy().to_string();

    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    let mut found = 0;
    for (i, line) in lines.iter().enumerate() {
        if found >= max_matches {
            break;
        }
        if re.is_match(line) {
            let before: Vec<String> = if ctx_before > 0 {
                let start = i.saturating_sub(ctx_before);
                lines[start..i].to_vec()
            } else {
                Vec::new()
            };

            let after: Vec<String> = if ctx_after > 0 {
                let end = (i + 1 + ctx_after).min(lines.len());
                lines[i + 1..end].to_vec()
            } else {
                Vec::new()
            };

            matches.push(GrepMatch {
                path: path_str.clone(),
                line: i + 1,
                content: line.clone(),
                context_before: before,
                context_after: after,
            });
            found += 1;
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

// ---------------------------------------------------------------------------
// Pattern sanitization — handle common LLM-generated pattern issues
// ---------------------------------------------------------------------------

/// Sanitize a regex pattern that may have been generated by an LLM.
///
/// Fixes common issues:
/// - Unescaped braces from template syntax (`${foo}`, `{bar}`)
/// - Unmatched braces that would cause regex parse errors
fn sanitize_pattern(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            '\\' => {
                // Already escaped — pass through both chars
                result.push('\\');
                if i + 1 < len {
                    i += 1;
                    result.push(chars[i]);
                }
            }
            '{' => {
                // Check if this looks like a valid regex quantifier: {N}, {N,}, {N,M}
                if let Some(close) = find_close_brace(&chars, i) {
                    let inner: String = chars[i + 1..close].iter().collect();
                    if is_valid_quantifier(&inner) {
                        // Valid quantifier — keep as-is
                        let chunk: String = chars[i..=close].iter().collect();
                        result.push_str(&chunk);
                        i = close;
                    } else {
                        // Not a quantifier (e.g. ${foo}) — escape the brace
                        result.push_str("\\{");
                    }
                } else {
                    // Unmatched opening brace — escape it
                    result.push_str("\\{");
                }
            }
            '}' => {
                // Stray closing brace — escape it
                result.push_str("\\}");
            }
            _ => result.push(chars[i]),
        }
        i += 1;
    }

    result
}

fn find_close_brace(chars: &[char], open: usize) -> Option<usize> {
    for (j, &ch) in chars.iter().enumerate().skip(open + 1).take(19) {
        if ch == '}' {
            return Some(j);
        }
        if ch == '{' {
            return None;
        }
    }
    None
}

fn is_valid_quantifier(inner: &str) -> bool {
    // Valid: "3", "3,", "3,7"
    let parts: Vec<&str> = inner.splitn(2, ',').collect();
    if parts.is_empty() || parts[0].is_empty() {
        return false;
    }
    if !parts[0].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if parts.len() == 2 && !parts[1].is_empty() && !parts[1].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

// sanitize_glob is in super::sanitize_glob (shared with glob tool)

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

    #[tokio::test]
    async fn grep_regex_support() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(
            dir.path().join("file.rs"),
            "fn hello() {}\nfn world() {}\nstruct Foo;",
        )
        .unwrap();

        let tool = GrepTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "fn \\w+"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("fn hello"));
        assert!(result.contains("fn world"));
        assert!(!result.contains("struct Foo"));
    }

    #[tokio::test]
    async fn grep_context_lines() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        std::fs::write(
            dir.path().join("file.txt"),
            "line1\nline2\nMATCH\nline4\nline5",
        )
        .unwrap();

        let tool = GrepTool;
        let result = tool
            .run(&serde_json::json!({
                "path": dir_str,
                "pattern": "MATCH",
                "context_before": 1,
                "context_after": 1
            }).to_string())
            .await
            .unwrap();

        assert!(result.contains("line2"));
        assert!(result.contains("MATCH"));
        assert!(result.contains("line4"));
    }

    #[tokio::test]
    async fn grep_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        // Init git repo so .gitignore is respected
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored/secret.txt"), "hello hidden").unwrap();
        std::fs::write(dir.path().join("visible.txt"), "hello visible").unwrap();

        let tool = GrepTool;
        let result = tool
            .run(&serde_json::json!({"path": dir_str, "pattern": "hello"}).to_string())
            .await
            .unwrap();

        assert!(result.contains("visible.txt"));
        assert!(!result.contains("secret.txt"));
    }

    #[test]
    fn sanitize_pattern_escapes_template_braces() {
        assert_eq!(sanitize_pattern("${foo}"), "$\\{foo\\}");
    }

    #[test]
    fn sanitize_pattern_keeps_valid_quantifiers() {
        assert_eq!(sanitize_pattern("a{3}"), "a{3}");
        assert_eq!(sanitize_pattern("a{3,7}"), "a{3,7}");
        assert_eq!(sanitize_pattern("a{3,}"), "a{3,}");
    }

    #[test]
    fn sanitize_pattern_escapes_unmatched() {
        assert_eq!(sanitize_pattern("a{b"), "a\\{b");
        assert_eq!(sanitize_pattern("a}b"), "a\\}b");
    }

    #[test]
    fn sanitize_glob_fixes_unclosed_braces() {
        assert_eq!(super::super::sanitize_glob("*.{ts,tsx"), "**/*.{ts,tsx}");
    }

    #[test]
    fn sanitize_glob_auto_prefixes() {
        assert_eq!(super::super::sanitize_glob("*.rs"), "**/*.rs");
        // Already has path separator — no prefix
        assert_eq!(super::super::sanitize_glob("src/*.rs"), "src/*.rs");
    }
}
