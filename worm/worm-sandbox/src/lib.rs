mod ast_guard;
mod constraint;
mod env_filter;
mod executor;
mod sandbox;
mod scrub;

mod noop;

#[cfg(target_os = "macos")]
mod seatbelt;

#[cfg(target_os = "linux")]
mod landlock;

pub use ast_guard::AstGuard;
pub use constraint::{ConstraintGuard, ToolConstraints};
pub use env_filter::{strip_blocked_env, strip_blocked_env_std, BLOCKED_ENV_VARS};
pub use executor::{
    create_sandbox, ExecutorError, NetworkPolicy, SandboxExecutor, SandboxPermissions,
};
pub use sandbox::{SandboxError, SandboxGuard, SandboxToolType};
pub use scrub::scrub_credentials;
