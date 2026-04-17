use std::io::Write as _;
use std::process::Output;
use std::time::Duration;

use crate::{NetworkPolicy, ExecutorError, SandboxExecutor, SandboxPermissions};

/// macOS Seatbelt sandbox executor.
///
/// Generates a `.sb` sandbox profile from permissions and launches
/// the command via `sandbox-exec -f <profile> -- sh -c <command>`.
/// The kernel enforces the profile — the sandboxed process cannot escape.
pub struct SeatbeltExecutor;

#[async_trait::async_trait]
impl SandboxExecutor for SeatbeltExecutor {
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        permissions: &SandboxPermissions,
        timeout: Duration,
    ) -> Result<Output, ExecutorError> {
        let profile = generate_profile(permissions);

        // Write profile to a temp file
        let mut tmp = tempfile::NamedTempFile::new()
            .map_err(|e| ExecutorError::Setup(format!("create temp profile: {e}")))?;
        tmp.write_all(profile.as_bytes())
            .map_err(|e| ExecutorError::Setup(format!("write profile: {e}")))?;
        let profile_path = tmp.into_temp_path();

        let mut cmd = tokio::process::Command::new("sandbox-exec");
        cmd.args([
            "-f",
            profile_path.to_str().unwrap_or("/dev/null"),
            "--",
            "sh",
            "-c",
            command,
        ]);
        cmd.current_dir(work_dir);
        crate::strip_blocked_env(&mut cmd);

        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| ExecutorError::Timeout(timeout))?
            .map_err(|e| ExecutorError::Command(e.to_string()))?;

        Ok(output)
    }

    fn backend_name(&self) -> &'static str {
        "seatbelt"
    }
}

/// Generates a Seatbelt .sb profile from permissions.
///
/// Strategy: deny default, allow all reads + process/mach/signal (needed for sh/dyld),
/// restrict writes to only allowed paths. This is the practical approach on macOS
/// since sh needs to read dyld cache, shared libs, etc. from many system paths.
fn generate_profile(perms: &SandboxPermissions) -> String {
    let mut lines = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        // Process operations (fork, exec)
        "(allow process*)".to_string(),
        // Read access to all files (sh needs dyld cache, shared libs, etc.)
        "(allow file-read*)".to_string(),
        // Sysctl for uname, etc.
        "(allow sysctl-read)".to_string(),
        // Mach IPC (needed for basic system operations)
        "(allow mach-lookup)".to_string(),
        // Signal handling
        "(allow signal)".to_string(),
    ];

    // Write access: only to explicitly allowed paths
    for path in &perms.write_paths {
        // Canonicalize to resolve symlinks (e.g., /tmp → /private/tmp on macOS)
        let canonical = std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.clone());
        let p = canonical.to_string_lossy();
        lines.push(format!("(allow file-write* (subpath \"{p}\"))"));
    }

    // Network
    match &perms.network {
        NetworkPolicy::DenyAll => {
            // Default deny covers this
        }
        NetworkPolicy::AllowEndpoints(endpoints) => {
            for ep in endpoints {
                lines.push(format!(
                    "(allow network-outbound (remote tcp \"{ep}\"))"
                ));
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn profile_generation_basic() {
        let perms = SandboxPermissions {
            read_paths: vec![PathBuf::from("/usr"), PathBuf::from("/bin")],
            write_paths: vec![PathBuf::from("/tmp")],
            network: NetworkPolicy::DenyAll,
        };

        let profile = generate_profile(&perms);
        assert!(profile.contains("(deny default)"));
        // Reads are globally allowed (sh needs dyld cache, shared libs)
        assert!(profile.contains("(allow file-read*)"));
        // Write restricted to specified paths (canonicalized)
        assert!(profile.contains("(allow file-write*"));
        assert!(!profile.contains("network-outbound"));
    }

    #[test]
    fn profile_with_network() {
        let perms = SandboxPermissions {
            read_paths: vec![],
            write_paths: vec![],
            network: NetworkPolicy::AllowEndpoints(vec![
                "api.anthropic.com:443".to_string(),
            ]),
        };

        let profile = generate_profile(&perms);
        assert!(profile.contains("api.anthropic.com:443"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn seatbelt_allows_echo() {
        let executor = SeatbeltExecutor;
        let perms = SandboxPermissions::for_workdir("/tmp");
        let output = executor
            .exec_sandboxed("echo hello", "/tmp", &perms, Duration::from_secs(5))
            .await
            .unwrap();

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "hello"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn seatbelt_blocks_write_outside_workdir() {
        let executor = SeatbeltExecutor;
        let perms = SandboxPermissions::for_workdir("/tmp/ozzie-sandbox-test");
        // Try to write outside the sandbox
        let output = executor
            .exec_sandboxed(
                "touch /tmp/ozzie-seatbelt-escape-test",
                "/tmp",
                &perms,
                Duration::from_secs(5),
            )
            .await
            .unwrap();

        // Command should fail due to sandbox denial
        assert!(!output.status.success() || !output.stderr.is_empty());
        // Clean up just in case
        let _ = std::fs::remove_file("/tmp/ozzie-seatbelt-escape-test");
    }
}
