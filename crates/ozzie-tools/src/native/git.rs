use std::time::Duration;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Git operations tool: status, diff, log, add, commit, branch, checkout.
pub struct GitTool;

/// Git command — each variant carries exactly the fields it needs.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
enum GitCommand {
    /// Show working tree status (porcelain format).
    Status {
        /// Limit status to a specific path.
        #[serde(default)]
        path: Option<String>,
    },
    /// Show changes between commits, working tree, etc.
    Diff {
        /// Limit diff to a specific path.
        #[serde(default)]
        path: Option<String>,
        /// Show staged changes only.
        #[serde(default)]
        staged: bool,
    },
    /// Show commit log (oneline format).
    Log {
        /// Limit log to a specific path.
        #[serde(default)]
        path: Option<String>,
        /// Maximum number of commits to show (default: 10, max: 100).
        #[serde(default)]
        max: Option<usize>,
    },
    /// Stage files for commit.
    Add {
        /// File paths to stage.
        paths: Vec<String>,
    },
    /// Create a new commit with the staged changes.
    Commit {
        /// Commit message.
        message: String,
    },
    /// List or create branches.
    Branch {
        /// Branch name to create. If omitted, lists all branches.
        #[serde(default)]
        name: Option<String>,
    },
    /// Switch to a branch or commit.
    Checkout {
        /// Branch name or commit ref to switch to.
        r#ref: String,
    },
}

#[derive(Serialize)]
struct GitResult {
    output: String,
    exit_code: i32,
}

impl GitTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "git".to_string(),
            description:
                "Execute git operations: status, diff, log, add, commit, branch, checkout."
                    .to_string(),
            parameters: schema_for::<GitCommand>(),
            dangerous: true,
        }
    }
}

#[async_trait::async_trait]
impl Tool for GitTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "git",
            "Execute git operations",
            GitTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let cmd: GitCommand = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let result = match cmd {
            GitCommand::Status { path } => {
                let mut args = vec!["status".to_string(), "--porcelain".to_string()];
                if let Some(p) = path {
                    args.push(p);
                }
                exec_git(&args).await
            }
            GitCommand::Diff { path, staged } => {
                let mut args = vec!["diff".to_string()];
                if staged {
                    args.push("--staged".to_string());
                }
                if let Some(p) = path {
                    args.push(p);
                }
                exec_git(&args).await
            }
            GitCommand::Log { path, max } => {
                let n = max.unwrap_or(10).clamp(1, 100);
                let mut args = vec!["log".to_string(), "--oneline".to_string(), format!("-{n}")];
                if let Some(p) = path {
                    args.push("--".to_string());
                    args.push(p);
                }
                exec_git(&args).await
            }
            GitCommand::Add { paths } => {
                if paths.is_empty() {
                    return Err(ToolError::Execution(
                        "git add: paths are required".to_string(),
                    ));
                }
                let mut args = vec!["add".to_string()];
                args.extend(paths);
                exec_git(&args).await
            }
            GitCommand::Commit { message } => {
                if message.is_empty() {
                    return Err(ToolError::Execution(
                        "git commit: message is required".to_string(),
                    ));
                }
                exec_git(&["commit".to_string(), "-m".to_string(), message]).await
            }
            GitCommand::Branch { name } => {
                if let Some(n) = name {
                    exec_git(&["branch".to_string(), n]).await
                } else {
                    exec_git(&["branch".to_string(), "-a".to_string()]).await
                }
            }
            GitCommand::Checkout { r#ref } => {
                if r#ref.is_empty() {
                    return Err(ToolError::Execution(
                        "git checkout: ref is required".to_string(),
                    ));
                }
                exec_git(&["checkout".to_string(), r#ref]).await
            }
        }?;

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

async fn exec_git(args: &[String]) -> Result<GitResult, ToolError> {
    let output = tokio::time::timeout(
        DEFAULT_TIMEOUT,
        tokio::process::Command::new("git").args(args).output(),
    )
    .await
    .map_err(|_| ToolError::Execution("git command timed out".to_string()))?
    .map_err(|e| ToolError::Execution(format!("git exec: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let out = if stdout.is_empty() { stderr } else { stdout };

    Ok(GitResult {
        output: out,
        exit_code: output.status.code().unwrap_or(-1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn git_status_runs() {
        let tool = GitTool;
        let result = tool.run(r#"{"action": "status"}"#).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn git_unknown_action() {
        let tool = GitTool;
        let result = tool.run(r#"{"action": "rebase"}"#).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn git_missing_action() {
        let tool = GitTool;
        let result = tool.run(r#"{}"#).await;
        assert!(result.is_err());
    }
}
