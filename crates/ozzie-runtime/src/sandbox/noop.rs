use std::process::Output;
use std::time::Duration;

use super::{SandboxError, SandboxExecutor, SandboxPermissions};

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
    ) -> Result<Output, SandboxError> {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);
        cmd.current_dir(work_dir);

        tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| SandboxError::Timeout(timeout))?
            .map_err(|e| SandboxError::Command(e.to_string()))
    }

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}
