/// Resolves a potentially relative path against the `work_dir` from `ToolContext`.
///
/// If the path is already absolute, it is returned as-is.
/// If `TOOL_CTX.work_dir` is set, relative paths are joined to it.
/// Otherwise the path is returned unchanged (resolved by the OS against cwd).
pub(crate) fn resolve_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    if let Ok(Some(wd)) = ozzie_core::domain::TOOL_CTX.try_with(|ctx| ctx.work_dir.clone()) {
        return std::path::Path::new(&wd).join(p).to_string_lossy().to_string();
    }
    path.to_string()
}

/// Enforces path jail: resolved path must be under `work_dir` (if set).
///
/// Only enforced when `TOOL_CTX.work_dir` is set (subtasks, scheduled tasks,
/// autonomous mode). Interactive sessions are unrestricted.
/// Resolves symlinks for existing paths to prevent escape via `../` or symlinks.
pub(crate) fn enforce_path_jail(resolved: &str) -> Result<(), ozzie_core::domain::ToolError> {
    let work_dir = match ozzie_core::domain::TOOL_CTX
        .try_with(|ctx| ctx.work_dir.clone())
        .ok()
        .flatten()
    {
        Some(wd) if !wd.is_empty() => wd,
        _ => return Ok(()), // No work_dir = no restriction
    };

    let canonical = best_effort_canonicalize(resolved);
    let canonical_work = best_effort_canonicalize(&work_dir);

    if canonical.starts_with(&canonical_work) {
        return Ok(());
    }

    Err(ozzie_core::domain::ToolError::Execution(format!(
        "path '{}' is outside work directory '{}'",
        resolved, work_dir
    )))
}

/// Canonicalizes a path, falling back to parent canonicalization if the path
/// doesn't exist yet. This handles macOS `/tmp` -> `/private/tmp` symlinks
/// correctly for new files.
pub(crate) fn best_effort_canonicalize(path: &str) -> String {
    // Fast path: file exists, full canonicalize resolves symlinks.
    if let Ok(p) = std::fs::canonicalize(path) {
        return p.to_string_lossy().to_string();
    }

    // Resolve `.` and `..` components first (logical resolution).
    use std::path::{Component, PathBuf};
    let mut logical = PathBuf::new();
    for component in std::path::Path::new(path).components() {
        match component {
            Component::ParentDir => { logical.pop(); }
            Component::CurDir => {}
            _ => logical.push(component),
        }
    }

    // Now canonicalize the longest existing ancestor to resolve symlinks,
    // then append the remaining (non-existing) tail.
    let mut ancestor = logical.clone();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        if let Some(name) = ancestor.file_name() {
            tail.push(name.to_os_string());
        }
        if !ancestor.pop() {
            break;
        }
    }
    let base = std::fs::canonicalize(&ancestor).unwrap_or(ancestor);
    let mut resolved = base;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    resolved.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::{ToolContext, TOOL_CTX};

    #[tokio::test]
    async fn path_jail_blocks_escape() {
        let ctx = ToolContext {
            work_dir: Some("/tmp/sandbox".to_string()),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { enforce_path_jail("/etc/passwd") })
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("outside work directory"));
    }

    #[tokio::test]
    async fn path_jail_allows_inside_workdir() {
        let ctx = ToolContext {
            work_dir: Some("/tmp/sandbox".to_string()),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async {
                enforce_path_jail("/tmp/sandbox/subdir/file.txt")
            })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn path_jail_blocks_dotdot_escape() {
        let ctx = ToolContext {
            work_dir: Some("/tmp/sandbox".to_string()),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async {
                enforce_path_jail("/tmp/sandbox/../../../etc/passwd")
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn path_jail_no_restriction_without_workdir() {
        let ctx = ToolContext::default();
        let result = TOOL_CTX
            .scope(ctx, async { enforce_path_jail("/etc/passwd") })
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn path_jail_allows_workdir_itself() {
        let ctx = ToolContext {
            work_dir: Some("/tmp/sandbox".to_string()),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { enforce_path_jail("/tmp/sandbox") })
            .await;
        assert!(result.is_ok());
    }
}
