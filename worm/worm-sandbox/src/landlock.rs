use std::os::unix::io::AsRawFd;
use std::process::Output;
use std::time::Duration;

use crate::{ExecutorError, SandboxExecutor, SandboxPermissions};

// ---- Landlock kernel ABI structs ----
// These are not exposed by the `libc` crate, so we define them here
// matching the kernel headers (include/uapi/linux/landlock.h).

#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
}

#[repr(C)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

// Syscall numbers — use libc constants when available, fallback for x86_64/aarch64.
// libc defines SYS_landlock_* for all Linux targets (glibc + musl, all arches).
use libc::{SYS_landlock_add_rule, SYS_landlock_create_ruleset, SYS_landlock_restrict_self};

// Rule type
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

/// Linux Landlock sandbox executor.
///
/// Uses the Landlock LSM (kernel 5.13+) to restrict filesystem access
/// of the child process. The restriction is applied after fork, before exec,
/// and is irrevocable.
pub struct LandlockExecutor;

/// Check if the running kernel supports Landlock.
pub fn is_supported() -> bool {
    unsafe {
        let attr = LandlockRulesetAttr {
            handled_access_fs: 0,
        };
        let fd = libc::syscall(
            SYS_landlock_create_ruleset,
            &attr as *const _,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32,
        );
        if fd >= 0 {
            libc::close(fd as i32);
            true
        } else {
            false
        }
    }
}

#[async_trait::async_trait]
impl SandboxExecutor for LandlockExecutor {
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        permissions: &SandboxPermissions,
        timeout: Duration,
    ) -> Result<Output, ExecutorError> {
        let perms = permissions.clone();

        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);
        cmd.current_dir(work_dir);
        crate::strip_blocked_env(&mut cmd);

        // SAFETY: pre_exec runs between fork and exec in the child process.
        // We only call async-signal-safe functions and Landlock syscalls.
        unsafe {
            let perms = perms.clone();
            cmd.pre_exec(move || {
                apply_landlock(&perms).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string())
                })
            });
        }

        tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| ExecutorError::Timeout(timeout))?
            .map_err(|e| ExecutorError::Command(e.to_string()))
    }

    fn backend_name(&self) -> &'static str {
        "landlock"
    }
}

/// Applies Landlock restrictions in the current process (child, after fork).
fn apply_landlock(perms: &SandboxPermissions) -> Result<(), ExecutorError> {
    // Access flags we want to control (Landlock ABI v1)
    const ACCESS_FS_ALL: u64 = (1 << 0)  // EXECUTE
        | (1 << 1)  // WRITE_FILE
        | (1 << 2)  // READ_FILE
        | (1 << 3)  // READ_DIR
        | (1 << 4)  // REMOVE_DIR
        | (1 << 5)  // REMOVE_FILE
        | (1 << 6)  // MAKE_CHAR
        | (1 << 7)  // MAKE_DIR
        | (1 << 8)  // MAKE_REG
        | (1 << 9)  // MAKE_SOCK
        | (1 << 10) // MAKE_FIFO
        | (1 << 11) // MAKE_BLOCK
        | (1 << 12); // MAKE_SYM

    const READ_ONLY: u64 = (1 << 0) // EXECUTE
        | (1 << 2) // READ_FILE
        | (1 << 3); // READ_DIR

    // Create ruleset
    let attr = LandlockRulesetAttr {
        handled_access_fs: ACCESS_FS_ALL,
    };
    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_landlock_create_ruleset,
            &attr as *const _,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32,
        )
    };
    if ruleset_fd < 0 {
        return Err(ExecutorError::Setup("landlock_create_ruleset failed".into()));
    }
    let ruleset_fd = ruleset_fd as i32;

    // Add read-only path rules
    for path in &perms.read_paths {
        if path.exists()
            && let Ok(file) = std::fs::File::open(path)
        {
            add_path_rule(ruleset_fd, file.as_raw_fd(), READ_ONLY);
        }
    }

    // Add read+write path rules
    for path in &perms.write_paths {
        if path.exists()
            && let Ok(file) = std::fs::File::open(path)
        {
            add_path_rule(ruleset_fd, file.as_raw_fd(), ACCESS_FS_ALL);
        }
    }

    // No new privileges (required before restrict_self)
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret != 0 {
        unsafe { libc::close(ruleset_fd) };
        return Err(ExecutorError::Setup(
            "prctl(PR_SET_NO_NEW_PRIVS) failed".into(),
        ));
    }

    // Restrict self
    let ret = unsafe {
        libc::syscall(SYS_landlock_restrict_self, ruleset_fd, 0u32)
    };
    unsafe { libc::close(ruleset_fd) };

    if ret != 0 {
        return Err(ExecutorError::Setup(
            "landlock_restrict_self failed".into(),
        ));
    }

    Ok(())
}

fn add_path_rule(ruleset_fd: i32, parent_fd: i32, access: u64) {
    let attr = LandlockPathBeneathAttr {
        allowed_access: access,
        parent_fd,
    };
    let ret = unsafe {
        libc::syscall(
            SYS_landlock_add_rule,
            ruleset_fd,
            LANDLOCK_RULE_PATH_BENEATH,
            &attr as *const _,
            0u32,
        )
    };
    if ret != 0 {
        // Non-fatal: path may not be mountable, continue
        tracing::debug!(fd = parent_fd, "landlock add_rule failed (non-fatal)");
    }
}
