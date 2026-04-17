mod permissions;
mod wrapper;

// Re-export from worm-sandbox — the canonical source of truth.
pub use worm_sandbox::{
    AstGuard, ConstraintGuard, SandboxError, SandboxGuard, SandboxToolType, ToolConstraints,
    scrub_credentials, strip_blocked_env, strip_blocked_env_std, BLOCKED_ENV_VARS,
};

// Ozzie-specific: tool permissions and dangerous tool approval flow.
pub use permissions::ToolPermissions;
pub use wrapper::{ApprovalRequester, ApprovalResponse, DangerousToolWrapper, prompt_label};
