use std::path::PathBuf;
use std::process::Output;

/// Permissions for a sandboxed command execution.
#[derive(Debug, Clone)]
pub struct SandboxPermissions {
    /// Paths with read access.
    pub read_paths: Vec<PathBuf>,
    /// Paths with read+write access.
    pub write_paths: Vec<PathBuf>,
    /// Network policy.
    pub network: NetworkPolicy,
}

impl Default for SandboxPermissions {
    fn default() -> Self {
        Self {
            read_paths: vec![
                PathBuf::from("/usr"),
                PathBuf::from("/bin"),
                PathBuf::from("/etc"),
                PathBuf::from("/lib"),
                PathBuf::from("/sbin"),
                PathBuf::from("/opt"),
            ],
            write_paths: Vec::new(),
            network: NetworkPolicy::DenyAll,
        }
    }
}

impl SandboxPermissions {
    /// Creates permissions scoped to a work directory (read+write)
    /// plus standard system paths (read-only).
    pub fn for_workdir(work_dir: &str) -> Self {
        let mut perms = Self::default();
        let wd = PathBuf::from(work_dir);
        perms.read_paths.push(wd.clone());
        perms.write_paths.push(wd);
        perms.write_paths.push(std::env::temp_dir());
        perms
    }
}

/// Network access policy for sandboxed commands.
#[derive(Debug, Clone)]
pub enum NetworkPolicy {
    /// Block all network access.
    DenyAll,
    /// Allow outbound to specific host:port pairs.
    AllowEndpoints(Vec<String>),
}

/// Executes a shell command inside an OS sandbox.
#[async_trait::async_trait]
pub trait SandboxExecutor: Send + Sync {
    /// Runs a command with restricted OS-level permissions.
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        permissions: &SandboxPermissions,
        timeout: std::time::Duration,
    ) -> Result<Output, ExecutorError>;

    /// Returns the sandbox backend name (for logging).
    fn backend_name(&self) -> &'static str;
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("sandbox setup failed: {0}")]
    Setup(String),
    #[error("command failed: {0}")]
    Command(String),
    #[error("command timed out after {0:?}")]
    Timeout(std::time::Duration),
}

/// Creates the best available sandbox for the current platform.
pub fn create_sandbox() -> Box<dyn SandboxExecutor> {
    #[cfg(target_os = "macos")]
    {
        Box::new(super::seatbelt::SeatbeltExecutor)
    }

    #[cfg(target_os = "linux")]
    {
        if super::landlock::is_supported() {
            Box::new(super::landlock::LandlockExecutor)
        } else {
            tracing::warn!("Landlock not supported on this kernel, using noop sandbox");
            Box::new(super::noop::NoopExecutor)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        tracing::warn!("no OS sandbox available on this platform");
        Box::new(super::noop::NoopExecutor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_permissions_have_system_paths() {
        let perms = SandboxPermissions::default();
        assert!(perms.read_paths.contains(&PathBuf::from("/usr")));
        assert!(perms.write_paths.is_empty());
    }

    #[test]
    fn workdir_permissions() {
        let perms = SandboxPermissions::for_workdir("/home/user/project");
        assert!(perms.write_paths.contains(&PathBuf::from("/home/user/project")));
        assert!(perms.read_paths.contains(&PathBuf::from("/home/user/project")));
        assert!(perms.read_paths.contains(&PathBuf::from("/usr")));
    }

    #[tokio::test]
    async fn create_sandbox_returns_executor() {
        let sandbox = create_sandbox();
        assert!(!sandbox.backend_name().is_empty());
    }
}
