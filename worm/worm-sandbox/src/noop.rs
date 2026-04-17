use std::process::Output;
use std::time::Duration;

use crate::{ExecutorError, SandboxExecutor, SandboxPermissions};

/// No-op sandbox: runs commands without OS-level isolation.
/// Used as fallback on unsupported platforms (Windows, old Linux kernels).
#[allow(dead_code)] // constructed conditionally per platform
pub struct NoopExecutor;

#[async_trait::async_trait]
impl SandboxExecutor for NoopExecutor {
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        _permissions: &SandboxPermissions,
        timeout: Duration,
    ) -> Result<Output, ExecutorError> {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);
        cmd.current_dir(work_dir);
        crate::strip_blocked_env(&mut cmd);

        tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| ExecutorError::Timeout(timeout))?
            .map_err(|e| ExecutorError::Command(e.to_string()))
    }

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}
