use std::path::{Path, PathBuf};

use super::ast_guard::AstGuard;

/// Sandbox tool types that determine validation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxToolType {
    /// Shell command execution.
    Exec,
    /// Filesystem operations.
    Filesystem,
}

/// Validates tool arguments against security policies.
///
/// Wraps a tool and checks commands/paths before execution.
/// Only enforced in autonomous mode; interactive mode passes through.
pub struct SandboxGuard {
    tool_name: String,
    tool_type: SandboxToolType,
    elevated: bool,
    allowed_paths: Vec<String>,
    ast_guard: AstGuard,
}

impl SandboxGuard {
    pub fn new(
        tool_name: &str,
        tool_type: SandboxToolType,
        elevated: bool,
        allowed_paths: Vec<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            tool_type,
            elevated,
            allowed_paths,
            ast_guard: AstGuard::new(),
        }
    }

    /// Validates a command string against the sandbox rules.
    pub fn validate_command(
        &self,
        command: &str,
        work_dir: &str,
        autonomous: bool,
    ) -> Result<(), SandboxError> {
        if !autonomous {
            return Ok(());
        }

        if self.elevated {
            return Err(SandboxError::Blocked(
                "elevated (sudo) commands are not allowed in autonomous mode".to_string(),
            ));
        }

        // AST-based semantic analysis (replaces naive denylist)
        self.ast_guard.validate(command)?;

        // Path jail: extract path-like arguments and check them
        let paths = extract_command_paths(command);
        for path in &paths {
            validate_path_in_workdir(work_dir, path, &self.allowed_paths)?;
        }

        Ok(())
    }

    /// Validates a filesystem path against the sandbox rules.
    pub fn validate_path(
        &self,
        path: &str,
        work_dir: &str,
        autonomous: bool,
    ) -> Result<(), SandboxError> {
        if !autonomous {
            return Ok(());
        }
        validate_path_in_workdir(work_dir, path, &self.allowed_paths)
    }

    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub fn tool_type(&self) -> SandboxToolType {
        self.tool_type
    }
}

/// Extracts path-like arguments from a command string (best-effort).
fn extract_command_paths(command: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in command.split_whitespace() {
        if token.starts_with('-') {
            continue;
        }
        if token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
        {
            paths.push(token.to_string());
        }
    }
    paths
}

/// Validates that a path is within work_dir or allowed_paths.
fn validate_path_in_workdir(
    work_dir: &str,
    path: &str,
    allowed_paths: &[String],
) -> Result<(), SandboxError> {
    // Resolve to absolute
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        PathBuf::from(work_dir).join(path)
    };

    // Best-effort canonicalization (resolve symlinks for existing paths)
    let canonical = eval_symlinks_existing(&resolved);
    let canonical_str = canonical.to_string_lossy();

    // Check work_dir
    if is_under(&canonical_str, work_dir) {
        return Ok(());
    }

    // Check allowed_paths
    for allowed in allowed_paths {
        if is_under(&canonical_str, allowed) {
            return Ok(());
        }
    }

    Err(SandboxError::PathViolation(format!(
        "path '{}' is outside work directory '{}'",
        path, work_dir
    )))
}

/// Resolves symlinks for the longest existing prefix of a path.
fn eval_symlinks_existing(path: &Path) -> PathBuf {
    // Try full canonicalize first
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }

    // Walk up until we find an existing prefix
    let mut existing = path.to_path_buf();
    let mut suffix = Vec::new();

    while !existing.exists() {
        if let Some(file_name) = existing.file_name() {
            suffix.push(file_name.to_owned());
        } else {
            break;
        }
        if !existing.pop() {
            break;
        }
    }

    let mut base = std::fs::canonicalize(&existing).unwrap_or(existing);
    for part in suffix.into_iter().rev() {
        base.push(part);
    }
    base
}

/// Returns true if `child` path is under `parent`.
fn is_under(child: &str, parent: &str) -> bool {
    let parent = parent.trim_end_matches('/');
    child == parent || child.starts_with(&format!("{parent}/"))
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("path violation: {0}")]
    PathViolation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_under_workdir() {
        assert!(is_under("/home/user/project/file.rs", "/home/user/project"));
        assert!(is_under("/home/user/project", "/home/user/project"));
        assert!(!is_under("/home/user/other/file.rs", "/home/user/project"));
    }

    #[test]
    fn extract_paths() {
        let paths = extract_command_paths("cat /etc/passwd && ls ./src -la");
        assert_eq!(paths, vec!["/etc/passwd", "./src"]);
    }

    #[test]
    fn sandbox_guard_blocks_elevated_in_autonomous() {
        let guard = SandboxGuard::new("root_cmd", SandboxToolType::Exec, true, vec![]);
        let err = guard.validate_command("apt install foo", "/tmp", true);
        assert!(err.is_err());
    }

    #[test]
    fn sandbox_guard_allows_in_interactive() {
        let guard = SandboxGuard::new("root_cmd", SandboxToolType::Exec, true, vec![]);
        assert!(guard.validate_command("apt install foo", "/tmp", false).is_ok());
    }

    #[test]
    fn sandbox_guard_delegates_to_ast() {
        let guard = SandboxGuard::new("exec", SandboxToolType::Exec, false, vec![]);
        assert!(guard.validate_command("sudo rm -rf /", "/tmp", true).is_err());
        assert!(guard.validate_command("echo hello", "/tmp", true).is_ok());
    }
}
