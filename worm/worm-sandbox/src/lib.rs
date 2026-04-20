mod ast_guard;
mod constraint;
mod env_filter;
mod executor;
mod regex_guard;
mod sandbox;
mod scrub;

mod noop;

#[cfg(target_os = "macos")]
mod seatbelt;

#[cfg(target_os = "linux")]
mod landlock;

#[cfg(target_os = "windows")]
mod windows;

pub use ast_guard::AstGuard;
pub use constraint::{ConstraintGuard, ToolConstraints};
pub use env_filter::{strip_blocked_env, strip_blocked_env_std, BLOCKED_ENV_VARS};
pub use executor::{
    create_sandbox, ExecutorError, NetworkPolicy, SandboxExecutor, SandboxPermissions,
};
pub use regex_guard::RegexGuard;
pub use sandbox::{SandboxError, SandboxGuard, SandboxToolType};
pub use scrub::scrub_credentials;
