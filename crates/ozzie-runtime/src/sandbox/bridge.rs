use ozzie_core::domain::{CommandSandbox, SandboxOutput, ToolError};
use worm_sandbox::{create_sandbox, SandboxExecutor, SandboxPermissions};

/// Bridge that implements the domain `CommandSandbox` port using a `SandboxExecutor`.
pub struct SandboxBridge {
    executor: Box<dyn SandboxExecutor>,
}

impl SandboxBridge {
    pub fn new(executor: Box<dyn SandboxExecutor>) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl CommandSandbox for SandboxBridge {
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        timeout: std::time::Duration,
    ) -> Result<SandboxOutput, ToolError> {
        let perms = SandboxPermissions::for_workdir(work_dir);
        let output = self
            .executor
            .exec_sandboxed(command, work_dir, &perms, timeout)
            .await
            .map_err(|e| ToolError::Execution(format!("sandbox: {e}")))?;
        Ok(SandboxOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    fn backend_name(&self) -> &'static str {
        self.executor.backend_name()
    }
}

/// Creates the best available sandbox for the current platform,
/// wrapped as a `CommandSandbox` domain port.
pub fn create_command_sandbox() -> Box<dyn CommandSandbox> {
    Box::new(SandboxBridge::new(create_sandbox()))
}
