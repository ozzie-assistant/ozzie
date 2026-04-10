use std::sync::Arc;
use std::time::Duration;

use ozzie_core::domain::{CommandSandbox, Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Shell command execution tool.
pub struct ExecuteTool {
    /// Optional OS-level sandbox. When set, commands are executed inside the sandbox.
    pub sandbox: Option<Arc<dyn CommandSandbox>>,
}

/// Arguments for the execute tool.
#[derive(Deserialize, JsonSchema)]
struct ExecuteArgs {
    /// Shell command to execute.
    command: String,
    /// Working directory (optional).
    #[serde(default)]
    working_dir: Option<String>,
    /// Run with elevated privileges.
    #[serde(default)]
    sudo: bool,
    /// Timeout in seconds (default: 30).
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Serialize, Deserialize)]
struct ExecuteResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

const EXECUTE_DESCRIPTION: &str = "\
Execute a shell command in a sandboxed environment. Returns stdout, stderr, and exit code. \
Restrictions: some system commands (ps, lsof, kill, netstat) may be blocked by OS sandbox. \
Redirections to system paths (/dev/null, /etc/) are blocked. \
Destructive commands (rm -rf, sudo) are blocked. \
If a command is blocked, use native tools instead: file_read, file_write, glob, grep, list_dir, web_fetch.";

impl ExecuteTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "execute".to_string(),
            description: EXECUTE_DESCRIPTION.to_string(),
            parameters: schema_for::<ExecuteArgs>(),
            dangerous: true,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ExecuteTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "execute",
            EXECUTE_DESCRIPTION,
            ExecuteTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let args: ExecuteArgs = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        // Sandbox: validate command against denylist
        let guard = ozzie_core::conscience::SandboxGuard::new(
            "execute",
            ozzie_core::conscience::SandboxToolType::Exec,
            args.sudo,
            Vec::new(),
        );
        let ctx_work_dir = ozzie_core::domain::TOOL_CTX
            .try_with(|ctx| ctx.work_dir.clone())
            .ok()
            .flatten();
        let work_dir_resolved = args.working_dir.as_deref()
            .or(ctx_work_dir.as_deref())
            .unwrap_or(".");
        let work_dir = work_dir_resolved;
        if let Err(e) = guard.validate_command(&args.command, work_dir, true) {
            return Err(ToolError::Execution(format!("sandbox blocked: {e}")));
        }

        // Tool constraints: validate command against task-level constraints
        if let Ok(ctx) = ozzie_core::domain::TOOL_CTX.try_with(|ctx| ctx.clone())
            && let Some(constraint) = ctx.tool_constraints.get("execute")
        {
                let tc = ozzie_core::conscience::ToolConstraints {
                    allowed_commands: constraint.allowed_commands.clone(),
                    allowed_patterns: constraint.allowed_patterns.clone(),
                    blocked_patterns: constraint.blocked_patterns.clone(),
                    allowed_paths: constraint.allowed_paths.clone(),
                    allowed_domains: constraint.allowed_domains.clone(),
                };
                let cg = ozzie_core::conscience::ConstraintGuard::new("execute", tc);
                if let Err(e) = cg.validate_command(&args.command) {
                    return Err(ToolError::Execution(format!("constraint violated: {e}")));
                }
        }

        // Emit progress so clients see what's running
        ozzie_core::domain::emit_progress("", "execute", &format!("$ {}", args.command));

        let timeout = Duration::from_secs(args.timeout);

        // Use OS sandbox if available and not elevated
        let output = if let Some(ref sandbox) = self.sandbox
            && !args.sudo
        {
            sandbox
                .exec_sandboxed(&args.command, work_dir, timeout)
                .await
                .map_err(|e| ToolError::Execution(format!("sandboxed exec: {e}")))?
        } else {
            let shell = if args.sudo { "sudo" } else { "sh" };
            let shell_args = if args.sudo {
                vec!["sh".to_string(), "-c".to_string(), args.command.clone()]
            } else {
                vec!["-c".to_string(), args.command.clone()]
            };

            let mut cmd = tokio::process::Command::new(shell);
            cmd.args(&shell_args);
            cmd.current_dir(work_dir);
            ozzie_core::conscience::strip_blocked_env(&mut cmd);

            tokio::time::timeout(timeout, cmd.output())
                .await
                .map_err(|_| ToolError::Execution(format!("command timed out after {}s", args.timeout)))?
                .map_err(|e| ToolError::Execution(format!("command failed: {e}")))?
        };

        let result = ExecuteResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        };

        // Detect OS-level sandbox blocks (Seatbelt on macOS, Landlock on Linux)
        // that surface as non-zero exit + characteristic stderr messages.
        // Return an explicit error so the LLM sees "Error: ..." and switches
        // strategy instead of retrying the same blocked command.
        if result.exit_code != 0 && is_os_sandbox_error(&result.stderr) {
            return Err(ToolError::Execution(format!(
                "OS sandbox blocked this command: {}. Use native tools instead \
                 (file_read, glob, grep, web_fetch).",
                result.stderr.trim()
            )));
        }

        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("serialize result: {e}")))
    }
}

/// Detect OS sandbox errors (Seatbelt/Landlock) in command stderr.
fn is_os_sandbox_error(stderr: &str) -> bool {
    stderr.contains("Operation not permitted")
        || stderr.contains("not allowed by")
        || stderr.contains("sandbox")
        || stderr.contains("permission denied")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_echo() {
        let tool = ExecuteTool { sandbox: None };
        let result = tool
            .run(r#"{"command": "echo hello"}"#)
            .await
            .unwrap();

        let parsed: ExecuteResult = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.stdout.trim(), "hello");
        assert_eq!(parsed.exit_code, 0);
    }

    #[tokio::test]
    async fn execute_invalid_args() {
        let tool = ExecuteTool { sandbox: None };
        let result = tool.run("not json").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn blocked_env_vars_not_leaked() {
        // Set a blocked var in our process, then verify the subprocess can't see it.
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test-secret") };

        let tool = ExecuteTool { sandbox: None };
        let result = tool
            .run(r#"{"command": "env"}"#)
            .await
            .unwrap();

        let parsed: ExecuteResult = serde_json::from_str(&result).unwrap();
        assert!(
            !parsed.stdout.contains("ANTHROPIC_API_KEY"),
            "blocked env var leaked to subprocess"
        );

        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    }
}
