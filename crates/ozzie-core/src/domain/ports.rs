use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::{Message, ModelTier};
use crate::connector::Identity;
use crate::policy::{Pairing, PairingKey};

// ---- Tool Context ----

/// A progress update from a running tool.
#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub call_id: String,
    pub tool: String,
    pub message: String,
}

/// Sender for tool progress updates (optional, set by runtime).
pub type ProgressSender = std::sync::Arc<dyn Fn(ToolProgress) + Send + Sync>;

/// Runtime context passed to tools via task-local storage.
///
/// Set by the EventRunner/ReactLoop before executing each tool call,
/// so context-aware tools (e.g. `activate`) can access session state
/// without changing the `Tool::run` signature.
#[derive(Clone, Default)]
pub struct ToolContext {
    /// Active session ID (empty if unknown).
    pub session_id: String,
    /// Per-tool constraints from task config (empty = no constraints).
    pub tool_constraints: HashMap<String, crate::events::ToolConstraint>,
    /// Working directory for resolving relative paths in tool calls.
    pub work_dir: Option<String>,
    /// Current subtask nesting depth (0 = top-level agent).
    pub subtask_depth: u32,
    /// Optional progress sender for long-running tools.
    pub progress: Option<ProgressSender>,
    /// Auto-commit file writes in the current workspace.
    pub git_auto_commit: bool,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("work_dir", &self.work_dir)
            .field("subtask_depth", &self.subtask_depth)
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

/// Emits a progress update from within a tool, if a sender is available.
pub fn emit_progress(call_id: &str, tool: &str, message: &str) {
    let _ = TOOL_CTX.try_with(|ctx| {
        if let Some(ref sender) = ctx.progress {
            sender(ToolProgress {
                call_id: call_id.to_string(),
                tool: tool.to_string(),
                message: message.to_string(),
            });
        }
    });
}

tokio::task_local! {
    /// Task-local tool context, set by the runtime before each tool call.
    pub static TOOL_CTX: ToolContext;
}

// ---- Tool Ports ----

/// Metadata describing a tool at the domain level.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub parameters: schemars::schema::RootSchema,
}

impl ToolInfo {
    /// Creates a ToolInfo with an empty parameters schema.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: empty_schema(),
        }
    }

    /// Creates a ToolInfo with the given parameters schema.
    pub fn with_parameters(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: schemars::schema::RootSchema,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

/// Returns an empty `{"type": "object", "properties": {}}` schema.
fn empty_schema() -> schemars::schema::RootSchema {
    use schemars::schema::*;
    RootSchema {
        schema: SchemaObject {
            instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
            object: Some(Box::new(ObjectValidation::default())),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Domain interface for any invokable tool.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Returns the tool's metadata.
    fn info(&self) -> ToolInfo;
    /// Executes the tool with JSON-encoded arguments.
    async fn run(&self, arguments_json: &str) -> Result<String, ToolError>;
}

/// Resolves tools by name from the registry.
pub trait ToolLookup: Send + Sync {
    fn tools_by_names(&self, names: &[String]) -> Vec<Box<dyn Tool>>;
    fn tool_names(&self) -> Vec<String>;
}

// ---- Runner Ports ----

/// Options for runner creation.
#[derive(Default)]
pub struct RunnerOpts {
    pub max_iterations: Option<usize>,
    /// Opaque adapter-specific middlewares.
    pub middlewares: Vec<Box<dyn std::any::Any + Send>>,
    /// Returns true when preemption is requested.
    pub preemption_check: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

/// Executes an agent turn.
#[async_trait::async_trait]
pub trait Runner: Send + Sync {
    async fn run(&self, messages: Vec<Message>) -> Result<String, RunnerError>;
}

/// Creates agent runners for a given model + tools.
#[async_trait::async_trait]
pub trait RunnerFactory: Send + Sync {
    async fn create_runner(
        &self,
        model: &str,
        instruction: &str,
        tools: Vec<Box<dyn Tool>>,
        opts: RunnerOpts,
    ) -> Result<Box<dyn Runner>, RunnerError>;
}

/// Non-streaming LLM call.
#[async_trait::async_trait]
pub trait Summarizer: Send + Sync {
    async fn summarize(&self, prompt: &str) -> Result<String, RunnerError>;
}

// ---- Tier Resolution ----

/// Maps provider names to model tiers.
pub trait TierResolver: Send + Sync {
    fn provider_tier(&self, name: &str) -> ModelTier;
}

// ---- Memory Port (re-exported from worm-memory) ----

pub use worm_memory::{
    MemoryEntryMeta, MemoryError, MemoryRetriever, MemorySearchEntry, MemoryStore, PageStore,
    RetrievedMemory,
};

// ---- Compression Port ----

/// Compresses a conversation history to fit within token budgets.
///
/// The default implementation is `LayeredContextCompressor` from `ozzie-runtime`,
/// backed by the L0/L1/L2 BM25 pipeline in `ozzie-core::layered`.
#[async_trait::async_trait]
pub trait ContextCompressor: Send + Sync {
    async fn compress(
        &self,
        session_id: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, CompressionError>;
}

// ---- Command Sandbox Port ----

/// Result of a sandboxed command execution.
#[derive(Debug, Clone)]
pub struct SandboxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[async_trait::async_trait]
pub trait CommandSandbox: Send + Sync {
    /// Runs a command with restricted OS-level permissions.
    /// `work_dir` is the only directory with write access by default.
    async fn exec_sandboxed(
        &self,
        command: &str,
        work_dir: &str,
        timeout: std::time::Duration,
    ) -> Result<SandboxOutput, ToolError>;

    /// Returns the sandbox backend name (for logging).
    fn backend_name(&self) -> &'static str;
}

// ---- Subtask Port ----

/// Runs a subtask inline via the ReAct loop.
///
/// Implemented in the gateway (or runtime) where the provider and tool registry
/// are available. Tools use this port to delegate sub-problems without needing
/// direct access to LLM infrastructure.
#[async_trait::async_trait]
pub trait SubtaskRunner: Send + Sync {
    async fn run_subtask(
        &self,
        instruction: &str,
        tools: &[String],
        work_dir: Option<&str>,
        subtask_depth: u32,
        provider: Option<&str>,
        tags: &[String],
    ) -> Result<String, ToolError>;
}

/// Runs a user-configured sub-agent as a one-shot ReactLoop.
///
/// The implementation lives in the gateway where the provider registry,
/// tool registry, and approval system are available.
#[async_trait::async_trait]
pub trait SubAgentRunner: Send + Sync {
    async fn run_sub_agent(
        &self,
        agent_name: &str,
        config: &crate::config::SubAgentConfig,
        task: &str,
        context: Option<&str>,
        session_id: &str,
        work_dir: Option<&str>,
    ) -> Result<String, ToolError>;
}

/// Runs a skill by name.
#[async_trait::async_trait]
pub trait SkillExecutor: Send + Sync {
    async fn run_skill(
        &self,
        skill_name: &str,
        vars: HashMap<String, String>,
    ) -> Result<String, SkillError>;
}

/// Seeds per-session tool permissions.
pub trait ToolPermissionsSeeder: Send + Sync {
    fn allow_for_session(&self, session_id: &str, tool_name: &str);
}

// ---- Pairing Ports ----

/// Domain port for chat connector pairing storage.
/// Maps `Identity` → `policy_name`. Default: `JsonPairingStore`.
pub trait PairingStorage: Send + Sync {
    fn add(&self, pairing: &Pairing) -> Result<(), PairingError>;
    /// Returns `true` if a matching entry was found and removed.
    fn remove(&self, key: &PairingKey) -> Result<bool, PairingError>;
    /// Resolves the most specific policy for an identity (exact → wildcard).
    fn resolve(&self, identity: &Identity) -> Option<String>;
    fn list(&self) -> Vec<Pairing>;
}

/// A paired device (remote TUI, webapp, Tauri, mobile).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceRecord {
    /// Gateway-generated unique device ID.
    pub device_id: String,
    /// Client type: "tui" | "webapp" | "tauri" | "mobile".
    pub client_type: String,
    /// Human-readable label (e.g. "MacBook Pro Michael").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Bearer token for WS auth.
    pub token: String,
    pub paired_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
}

/// Domain port for device pairing storage.
/// Default: `JsonDeviceStore` persisted to `$OZZIE_PATH/devices.json`.
pub trait DeviceStorage: Send + Sync {
    fn add(&self, record: DeviceRecord) -> Result<(), PairingError>;
    /// Returns the record if the token is valid (or `None` if unknown).
    fn verify_token(&self, token: &str) -> Option<DeviceRecord>;
    fn list(&self) -> Vec<DeviceRecord>;
    /// Returns `true` if the device was found and revoked.
    fn revoke(&self, device_id: &str) -> Result<bool, PairingError>;
    /// Updates `last_seen` timestamp for a device.
    fn touch(&self, device_id: &str) -> Result<(), PairingError>;
}

/// A pending pairing request (device or chat).
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub request_id: String,
    pub kind: PendingKind,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Discriminates between device and chat pending requests.
#[derive(Debug, Clone)]
pub enum PendingKind {
    Device {
        client_type: String,
        label: Option<String>,
    },
    Chat {
        identity: Identity,
        display_name: String,
        /// Raw platform message for admin context.
        platform_message: String,
    },
}

/// In-memory TTL store for pending pairing requests (both device and chat flows).
/// Default: `MemoryPendingPairings` — no persistence needed (requests expire after TTL).
pub trait PendingPairings: Send + Sync {
    fn insert(&self, req: PendingRequest);
    fn list(&self) -> Vec<PendingRequest>;
    /// Consumes and returns the request on success (used during approval).
    fn take(&self, request_id: &str) -> Option<PendingRequest>;
    /// Removes all expired requests.
    fn purge_expired(&self);
}

// ---- Blob Storage Port ----

/// Stores and retrieves binary blobs (images, etc.) by content hash.
///
/// Blobs live in `{session_dir}/blobs/{hash}.{ext}`. The store is keyed
/// by SHA-256 so identical content deduplicates automatically.
#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
    /// Writes bytes to the store and returns the content-addressed reference.
    async fn write(&self, bytes: &[u8], media_type: &str) -> Result<ozzie_types::BlobRef, BlobError>;
    /// Reads the raw bytes for a blob reference.
    async fn read(&self, blob: &ozzie_types::BlobRef) -> Result<Vec<u8>, BlobError>;
    /// Returns true if the blob exists in the store.
    async fn exists(&self, blob: &ozzie_types::BlobRef) -> bool;
}

// ---- Errors ----

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("blob not found: {0}")]
    NotFound(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("{0}")]
    Io(String),
    #[error("not found: {0}")]
    NotFound(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("tool execution failed: {0}")]
    Execution(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("runner preempted")]
    Preempted,
    #[error("model unavailable ({provider}): {cause}")]
    ModelUnavailable { provider: String, cause: String },
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("skill execution failed: {0}")]
    Execution(String),
    #[error("{0}")]
    Other(String),
}
