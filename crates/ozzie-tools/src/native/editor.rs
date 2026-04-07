use std::collections::HashMap;
use std::sync::Mutex;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

const MAX_OUTPUT_LINES: usize = 300;
const SNIPPET_RADIUS: usize = 4;

/// Rich file editor with view, create, str_replace, insert, and undo operations.
pub struct StrReplaceEditorTool {
    /// Undo history: path → stack of previous contents.
    history: Mutex<HashMap<String, Vec<String>>>,
}

impl Default for StrReplaceEditorTool {
    fn default() -> Self {
        Self::new()
    }
}

impl StrReplaceEditorTool {
    pub fn new() -> Self {
        Self {
            history: Mutex::new(HashMap::new()),
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "str_replace_editor".to_string(),
            description: "A rich file editor. Commands: view (show file with line numbers or list directory), create (create new file), str_replace (replace unique string), insert (insert text after line), undo_edit (undo last edit).".to_string(),
            parameters: schema_for::<EditorCommand>(),
            dangerous: true,
        }
    }

    fn push_history(&self, path: &str, content: &str) {
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history
            .entry(path.to_string())
            .or_default()
            .push(content.to_string());
    }

    fn pop_history(&self, path: &str) -> Option<String> {
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history.get_mut(path).and_then(|stack| stack.pop())
    }
}

/// Editor command — each variant carries exactly the fields it needs.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "command", rename_all = "snake_case")]
enum EditorCommand {
    /// Show file contents with line numbers, or list a directory.
    View {
        /// Absolute or relative path to the file or directory.
        path: String,
        /// Optional [start_line, end_line] range (1-based inclusive).
        #[serde(default)]
        view_range: Option<Vec<usize>>,
    },
    /// Create a new file (fails if it already exists).
    Create {
        /// Absolute or relative path for the new file.
        path: String,
        /// Content to write into the file.
        #[serde(default)]
        file_text: Option<String>,
    },
    /// Replace a unique string occurrence in a file.
    StrReplace {
        /// Path to the file to edit.
        path: String,
        /// String to find (must appear exactly once).
        old_str: String,
        /// Replacement string.
        #[serde(default)]
        new_str: Option<String>,
    },
    /// Insert text after a given line number.
    Insert {
        /// Path to the file to edit.
        path: String,
        /// Line number after which to insert (0 = beginning of file).
        insert_line: usize,
        /// Text to insert.
        #[serde(default)]
        new_text: Option<String>,
    },
    /// Undo the last edit on a file.
    UndoEdit {
        /// Path to the file to undo.
        path: String,
    },
}

#[async_trait::async_trait]
impl Tool for StrReplaceEditorTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "str_replace_editor",
            "Rich file editor with view, create, str_replace, insert, and undo",
            StrReplaceEditorTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let cmd: EditorCommand = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        match cmd {
            EditorCommand::View { path, view_range } => self.cmd_view(&path, view_range).await,
            EditorCommand::Create { path, file_text } => self.cmd_create(&path, file_text).await,
            EditorCommand::StrReplace {
                path,
                old_str,
                new_str,
            } => self.cmd_str_replace(&path, &old_str, new_str).await,
            EditorCommand::Insert {
                path,
                insert_line,
                new_text,
            } => self.cmd_insert(&path, insert_line, new_text).await,
            EditorCommand::UndoEdit { path } => self.cmd_undo(&path).await,
        }
    }
}

impl StrReplaceEditorTool {
    async fn cmd_view(
        &self,
        path: &str,
        view_range: Option<Vec<usize>>,
    ) -> Result<String, ToolError> {
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| ToolError::Execution(format!("view: path '{path}': {e}")))?;

        if meta.is_dir() {
            return self.list_dir(path).await;
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::Execution(format!("view: read '{path}': {e}")))?;

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let mut start = 1usize;
        let mut end = total;

        if let Some(range) = &view_range {
            if !range.is_empty() {
                start = range[0].max(1);
            }
            if range.len() >= 2 {
                end = range[1];
            }
        }

        if end > total {
            end = total;
        }
        if start > end {
            return Err(ToolError::Execution(format!(
                "view: invalid range [{start}, {end}]"
            )));
        }

        Ok(make_output(&lines[start - 1..end], path, start))
    }

    async fn list_dir(&self, path: &str) -> Result<String, ToolError> {
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::Execution(format!("view: list dir '{path}': {e}")))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| ToolError::Execution(format!("view: read entry: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "vendor" {
                continue;
            }
            let meta = entry.metadata().await;
            let suffix = if meta.is_ok_and(|m| m.is_dir()) {
                "/"
            } else {
                ""
            };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort();
        if entries.is_empty() {
            return Ok("Directory is empty.".to_string());
        }
        Ok(entries.join("\n"))
    }

    async fn cmd_create(
        &self,
        path: &str,
        file_text: Option<String>,
    ) -> Result<String, ToolError> {
        if tokio::fs::metadata(path).await.is_ok() {
            return Err(ToolError::Execution(format!(
                "create: file '{path}' already exists — use str_replace or insert to edit it"
            )));
        }

        let content = file_text.as_deref().unwrap_or("");

        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("create: mkdir: {e}")))?;
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| ToolError::Execution(format!("create: write '{path}': {e}")))?;

        let lines: Vec<&str> = content.lines().collect();
        Ok(make_output(&lines, path, 1))
    }

    async fn cmd_str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: Option<String>,
    ) -> Result<String, ToolError> {
        let new_str = new_str.as_deref().unwrap_or("");

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::Execution(format!("str_replace: read '{path}': {e}")))?;

        let count = content.matches(old_str).count();
        if count == 0 {
            return Err(ToolError::Execution(format!(
                "str_replace: old_str not found in {path}"
            )));
        }
        if count > 1 {
            return Err(ToolError::Execution(format!(
                "str_replace: old_str appears {count} times in {path} — must be unique"
            )));
        }

        self.push_history(path, &content);

        let new_content = content.replacen(old_str, new_str, 1);

        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| ToolError::Execution(format!("str_replace: write '{path}': {e}")))?;

        Ok(snippet_around(&new_content, new_str, path))
    }

    async fn cmd_insert(
        &self,
        path: &str,
        line: usize,
        new_text: Option<String>,
    ) -> Result<String, ToolError> {
        let text = new_text.as_deref().unwrap_or("");

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::Execution(format!("insert: read '{path}': {e}")))?;

        let lines: Vec<&str> = content.lines().collect();
        if line > lines.len() {
            return Err(ToolError::Execution(format!(
                "insert: line {line} out of range [0, {}]",
                lines.len()
            )));
        }

        self.push_history(path, &content);

        let insert_lines: Vec<&str> = text.lines().collect();
        let mut new_lines: Vec<&str> = Vec::with_capacity(lines.len() + insert_lines.len());
        new_lines.extend_from_slice(&lines[..line]);
        new_lines.extend_from_slice(&insert_lines);
        new_lines.extend_from_slice(&lines[line..]);

        let new_content = new_lines.join("\n");

        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| ToolError::Execution(format!("insert: write '{path}': {e}")))?;

        let snippet_start = line + 1;
        let snippet_end = line + insert_lines.len();
        Ok(snippet_range(&new_lines, path, snippet_start, snippet_end))
    }

    async fn cmd_undo(&self, path: &str) -> Result<String, ToolError> {
        let prev = self
            .pop_history(path)
            .ok_or_else(|| ToolError::Execution(format!("undo_edit: no edit history for {path}")))?;

        tokio::fs::write(path, &prev)
            .await
            .map_err(|e| ToolError::Execution(format!("undo_edit: write '{path}': {e}")))?;

        let lines: Vec<&str> = prev.lines().collect();
        Ok(make_output(&lines, path, 1))
    }
}

fn make_output(lines: &[&str], label: &str, start_line: usize) -> String {
    let mut buf = String::new();
    let capped = if lines.len() > MAX_OUTPUT_LINES {
        buf.push_str(&format!(
            "[Showing first {MAX_OUTPUT_LINES} lines of {label}]\n"
        ));
        &lines[..MAX_OUTPUT_LINES]
    } else {
        lines
    };
    for (i, line) in capped.iter().enumerate() {
        buf.push_str(&format!("{:6}\t{}\n", start_line + i, line));
    }
    buf
}

fn snippet_around(content: &str, needle: &str, label: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some(idx) = content.find(needle) else {
        return make_output(&lines, label, 1);
    };
    let line_num = content[..idx].matches('\n').count();
    let needle_lines = needle.matches('\n').count() + 1;

    let start = line_num.saturating_sub(SNIPPET_RADIUS);
    let end = (line_num + needle_lines + SNIPPET_RADIUS).min(lines.len());

    make_output(&lines[start..end], label, start + 1)
}

fn snippet_range(lines: &[&str], label: &str, start_line: usize, end_line: usize) -> String {
    let start = start_line.saturating_sub(SNIPPET_RADIUS + 1);
    let end = (end_line + SNIPPET_RADIUS).min(lines.len());
    make_output(&lines[start..end], label, start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_view() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let tool = StrReplaceEditorTool::new();

        let result = tool
            .run(&serde_json::json!({
                "command": "create", "path": path_str, "file_text": "hello\nworld"
            }).to_string())
            .await
            .unwrap();
        assert!(result.contains("hello"));

        let result = tool
            .run(&serde_json::json!({"command": "view", "path": path_str}).to_string())
            .await
            .unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[tokio::test]
    async fn str_replace_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replace.txt");
        let path_str = path.to_str().unwrap();

        let tool = StrReplaceEditorTool::new();

        tool.run(&serde_json::json!({
            "command": "create", "path": path_str, "file_text": "foo bar baz"
        }).to_string())
        .await
        .unwrap();

        tool.run(&serde_json::json!({
            "command": "str_replace", "path": path_str, "old_str": "bar", "new_str": "qux"
        }).to_string())
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(path_str).await.unwrap();
        assert_eq!(content, "foo qux baz");

        tool.run(&serde_json::json!({"command": "undo_edit", "path": path_str}).to_string())
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(path_str).await.unwrap();
        assert_eq!(content, "foo bar baz");
    }

    #[tokio::test]
    async fn insert_at_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("insert.txt");
        let path_str = path.to_str().unwrap();

        let tool = StrReplaceEditorTool::new();

        tool.run(&serde_json::json!({
            "command": "create", "path": path_str, "file_text": "line1\nline2\nline3"
        }).to_string())
        .await
        .unwrap();

        tool.run(&serde_json::json!({
            "command": "insert", "path": path_str, "insert_line": 1, "new_text": "inserted"
        }).to_string())
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(path_str).await.unwrap();
        assert_eq!(content, "line1\ninserted\nline2\nline3");
    }

    #[tokio::test]
    async fn view_directory() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_str().unwrap();

        tokio::fs::write(dir.path().join("a.txt"), "content")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("b.txt"), "content")
            .await
            .unwrap();

        let tool = StrReplaceEditorTool::new();
        let result = tool
            .run(&serde_json::json!({"command": "view", "path": dir_str}).to_string())
            .await
            .unwrap();

        assert!(result.contains("a.txt"));
        assert!(result.contains("b.txt"));
    }

    #[tokio::test]
    async fn create_existing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exists.txt");
        let path_str = path.to_str().unwrap();

        tokio::fs::write(&path, "content").await.unwrap();

        let tool = StrReplaceEditorTool::new();
        let result = tool
            .run(&serde_json::json!({
                "command": "create", "path": path_str, "file_text": "new"
            }).to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn str_replace_not_unique_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.txt");
        let path_str = path.to_str().unwrap();

        let tool = StrReplaceEditorTool::new();
        tool.run(&serde_json::json!({
            "command": "create", "path": path_str, "file_text": "aaa aaa"
        }).to_string())
        .await
        .unwrap();

        let result = tool
            .run(&serde_json::json!({
                "command": "str_replace", "path": path_str, "old_str": "aaa", "new_str": "bbb"
            }).to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn unknown_command_fails() {
        let tool = StrReplaceEditorTool::new();
        let result = tool
            .run(r#"{"command": "delete", "path": "/tmp/x"}"#)
            .await;
        assert!(result.is_err());
    }
}
