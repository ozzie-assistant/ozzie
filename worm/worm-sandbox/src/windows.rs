//! Windows sandbox executor — RegexGuard + env filtering.
//!
//! No OS-level isolation (no Windows equivalent of Seatbelt/Landlock),
//! but applies regex-based command validation and environment scrubbing.

use std::process::Output;
use std::time::Duration;

use crate::regex_guard::RegexGuard;
use crate::{ExecutorError, SandboxExecutor, SandboxPermissions};

/// Windows sandbox: regex guard + env filtering, no OS-level isolation.
pub struct WindowsExecutor {
    guard: RegexGuard,
}

impl WindowsExecutor {
    pub fn new() -> Self {
        Self {
            guard: RegexGuard::new(),
        }
    }
}

impl Default for WindowsExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SandboxExecutor for WindowsExecutor {
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        _permissions: &SandboxPermissions,
        timeout: Duration,
    ) -> Result<Output, ExecutorError> {
        // Validate command against regex patterns
        self.guard
            .validate(command)
            .map_err(|e| ExecutorError::Setup(e.to_string()))?;

        // Execute via cmd.exe
        let mut cmd = tokio::process::Command::new("cmd.exe");
        cmd.args(["/C", command]);
        cmd.current_dir(work_dir);
        crate::strip_blocked_env(&mut cmd);

        tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| ExecutorError::Timeout(timeout))?
            .map_err(|e| ExecutorError::Command(e.to_string()))
    }

    fn backend_name(&self) -> &'static str {
        "windows-regex"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_name() {
        let executor = WindowsExecutor::new();
        assert_eq!(executor.backend_name(), "windows-regex");
    }

    #[test]
    fn guard_blocks_dangerous_before_exec() {
        let executor = WindowsExecutor::new();
        // Validation is sync — we can test it without actually running the command
        assert!(executor.guard.validate("del /f /s /q C:\\*").is_err());
        assert!(executor.guard.validate("dir").is_ok());
    }
}
