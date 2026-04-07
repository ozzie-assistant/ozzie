use std::collections::HashMap;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Root configuration for Ozzie.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub events: EventsConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub layered_context: LayeredContextConfig,
    #[serde(default)]
    pub policies: PoliciesConfig,
    #[serde(default)]
    pub connectors: ConnectorsConfig,
    #[serde(default)]
    pub sub_agents: SubAgentsConfig,
}

impl Config {
    /// Deserializes a connector-specific config section by key.
    ///
    /// Returns `None` if the key is absent, or an error if deserialization fails.
    pub fn connector_config<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, String> {
        match self.connectors.0.get(key) {
            None => Ok(None),
            Some(cpc) => {
                let value = cpc.config.clone().unwrap_or(serde_json::Value::Null);
                serde_json::from_value(value)
                    .map(Some)
                    .map_err(|e| format!("invalid connector config '{key}': {e}"))
            }
        }
    }
}

// ---- Gateway ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    18420
}

// ---- Models ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelsConfig {
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

fn is_default_concurrent(v: &u32) -> bool {
    *v == 1
}

// ── Driver ───────────────────────────────────────────────────────────────

/// LLM driver — identifies which provider backend to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Driver {
    #[serde(rename = "anthropic")]
    #[default]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "mistral")]
    Mistral,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "groq")]
    Groq,
    #[serde(rename = "xai")]
    Xai,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "lm-studio")]
    LmStudio,
    #[serde(rename = "vllm")]
    Vllm,
}

impl Driver {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Mistral => "mistral",
            Self::Ollama => "ollama",
            Self::Groq => "groq",
            Self::Xai => "xai",
            Self::OpenAiCompatible => "openai-compatible",
            Self::LmStudio => "lm-studio",
            Self::Vllm => "vllm",
        }
    }

    /// Human-readable label for UI display.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Gemini (Google)",
            Self::Mistral => "Mistral",
            Self::Ollama => "Ollama (local)",
            Self::Groq => "Groq",
            Self::Xai => "xAI (Grok)",
            Self::OpenAiCompatible => "OpenAI-compatible (custom)",
            Self::LmStudio => "LM Studio",
            Self::Vllm => "vLLM",
        }
    }

    /// Environment variable name for the API key.
    pub const fn env_var(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Mistral => "MISTRAL_API_KEY",
            Self::Ollama => "OLLAMA_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Xai => "XAI_API_KEY",
        }
    }

    /// Whether this driver requires an API key.
    ///
    /// Returns false for local/self-hosted drivers where auth is optional.
    pub const fn needs_api_key(self) -> bool {
        !matches!(
            self,
            Self::Ollama | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm
        )
    }

    /// Whether this driver needs a custom base URL.
    pub const fn needs_base_url(self) -> bool {
        matches!(self, Self::Ollama | Self::OpenAiCompatible | Self::LmStudio | Self::Vllm)
    }

    /// Default base URL, if any.
    pub const fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("http://localhost:11434"),
            _ => None,
        }
    }

    /// Drivers available for LLM provider configuration.
    pub const ALL_LLM: &[Driver] = &[
        Self::Anthropic,
        Self::OpenAi,
        Self::Gemini,
        Self::Mistral,
        Self::Groq,
        Self::Xai,
        Self::Ollama,
        Self::OpenAiCompatible,
    ];

    /// Drivers available for embedding configuration (no Anthropic).
    pub const ALL_EMBEDDING: &[Driver] = &[
        Self::OpenAi,
        Self::Mistral,
        Self::Gemini,
        Self::Ollama,
        Self::OpenAiCompatible,
    ];
}

impl std::fmt::Display for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Driver {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::OpenAi),
            "gemini" => Ok(Self::Gemini),
            "mistral" => Ok(Self::Mistral),
            "ollama" => Ok(Self::Ollama),
            "groq" => Ok(Self::Groq),
            "xai" => Ok(Self::Xai),
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            "lm-studio" => Ok(Self::LmStudio),
            "vllm" => Ok(Self::Vllm),
            other => Err(format!("unknown driver: {other}")),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub driver: Driver,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "auth_is_empty")]
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<usize>,
    #[serde(default = "default_max_concurrent", skip_serializing_if = "is_default_concurrent")]
    pub max_concurrent: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<crate::domain::ModelCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "option_duration_serde")]
    pub timeout: Option<Duration>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub options: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// Turn budget overrides for this provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetConfig>,
}

/// Per-provider turn budget configuration.
/// All fields optional — unset fields use the default TurnBudget values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Max LLM calls per turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    /// Max cumulative output tokens per turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Timeout in seconds per turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_sec: Option<u64>,
}

fn default_max_concurrent() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default)]
    pub max_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "option_duration_serde")]
    pub initial_delay: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "option_duration_serde")]
    pub max_delay: Option<Duration>,
    #[serde(default)]
    pub multiplier: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.api_key.is_none() && self.token.is_none()
    }
}

fn auth_is_empty(auth: &AuthConfig) -> bool {
    auth.is_empty()
}

// ---- Events ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsConfig {
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for EventsConfig {
    fn default() -> Self {
        Self {
            buffer_size: default_buffer_size(),
            log_level: default_log_level(),
        }
    }
}

fn default_buffer_size() -> usize {
    1024
}

fn default_log_level() -> String {
    "info".to_string()
}

// ---- Agent ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_language: Option<String>,
}

// ---- Embedding ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<Driver>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dims: Option<usize>,
    #[serde(default, skip_serializing_if = "auth_is_empty")]
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_size: Option<usize>,
}

impl EmbeddingConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

// ---- Plugins ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub authorizations: HashMap<String, PluginAuthorizationConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginAuthorizationConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<FsAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<SecretsAuthConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceLimitsConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpAuthConfig {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FsAuthConfig {
    #[serde(default)]
    pub allowed_paths: HashMap<String, String>,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretsAuthConfig {
    #[serde(default)]
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_max_pages: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

// ---- Skills ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub dirs: Vec<String>,
    #[serde(default)]
    pub enabled: Vec<String>,
}

// ---- Tools ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub allowed_dangerous: Vec<String>,
}

// ---- Sandbox ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

// ---- Runtime ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_tools_file: Option<String>,
}

// ---- Web ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default)]
    pub search: WebSearchConfig,
    #[serde(default)]
    pub fetch: WebFetchConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default)]
    pub max_results: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google_cx: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bing_api_key: Option<String>,
}

impl WebSearchConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebFetchConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default)]
    pub max_body_kb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

impl WebFetchConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

// ---- MCP ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum McpServerConfig {
    Stdio {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
        #[serde(default)]
        dangerous: Option<bool>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        denied_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        trusted_tools: Vec<String>,
        #[serde(default = "default_mcp_timeout")]
        timeout: u64,
    },
    Sse {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default)]
        dangerous: Option<bool>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        denied_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        trusted_tools: Vec<String>,
        #[serde(default = "default_mcp_timeout")]
        timeout: u64,
    },
    Http {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default)]
        dangerous: Option<bool>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        denied_tools: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        trusted_tools: Vec<String>,
        #[serde(default = "default_mcp_timeout")]
        timeout: u64,
    },
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self::Stdio {
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            dangerous: None,
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            trusted_tools: Vec::new(),
            timeout: default_mcp_timeout(),
        }
    }
}

fn default_mcp_timeout() -> u64 {
    30000
}

impl McpServerConfig {
    /// Returns true if the server's tools should be marked dangerous (default: true).
    pub fn is_dangerous(&self) -> bool {
        let dangerous = match self {
            Self::Stdio { dangerous, .. }
            | Self::Sse { dangerous, .. }
            | Self::Http { dangerous, .. } => dangerous,
        };
        dangerous.unwrap_or(true)
    }

    /// Returns the allowed tools list.
    pub fn allowed_tools(&self) -> &[String] {
        match self {
            Self::Stdio { allowed_tools, .. }
            | Self::Sse { allowed_tools, .. }
            | Self::Http { allowed_tools, .. } => allowed_tools,
        }
    }

    /// Returns the denied tools list.
    pub fn denied_tools(&self) -> &[String] {
        match self {
            Self::Stdio { denied_tools, .. }
            | Self::Sse { denied_tools, .. }
            | Self::Http { denied_tools, .. } => denied_tools,
        }
    }

    /// Returns the trusted tools list.
    pub fn trusted_tools(&self) -> &[String] {
        match self {
            Self::Stdio { trusted_tools, .. }
            | Self::Sse { trusted_tools, .. }
            | Self::Http { trusted_tools, .. } => trusted_tools,
        }
    }

    /// Returns the timeout in milliseconds.
    pub fn timeout(&self) -> u64 {
        match self {
            Self::Stdio { timeout, .. }
            | Self::Sse { timeout, .. }
            | Self::Http { timeout, .. } => *timeout,
        }
    }
}

// ---- Layered Context ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredContextConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default = "default_max_archives")]
    pub max_archives: usize,
    #[serde(default = "default_max_recent_messages")]
    pub max_recent_messages: usize,
    #[serde(default = "default_archive_chunk_size")]
    pub archive_chunk_size: usize,
}

impl Default for LayeredContextConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            max_archives: default_max_archives(),
            max_recent_messages: default_max_recent_messages(),
            archive_chunk_size: default_archive_chunk_size(),
        }
    }
}

fn default_max_archives() -> usize {
    12
}

fn default_max_recent_messages() -> usize {
    24
}

fn default_archive_chunk_size() -> usize {
    8
}

impl LayeredContextConfig {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

// ---- Policies ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoliciesConfig {
    #[serde(default)]
    pub overrides: HashMap<String, PolicyOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyOverride {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_facing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,
}

// ---- Connectors ----

/// Connector process configuration — each entry describes a child process to supervise.
///
/// Access connector-specific config (the opaque `config` field) via [`Config::connector_config`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectorsConfig(pub HashMap<String, ConnectorProcessConfig>);

/// Configuration for a single connector child process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorProcessConfig {
    /// Binary to execute.
    #[serde(default)]
    pub command: String,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables (templates resolved before spawn).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Connector-specific configuration (passed as OZZIE_CONNECTOR_CONFIG env var, JSON-encoded).
    /// Allows each connector to have its own config shape (e.g. Discord token, File paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    /// Auto-pair this connector (same .key = auto-approve). Default: true.
    #[serde(default = "bool_true")]
    pub auto_pair: bool,
    /// Restart on crash. Default: false.
    #[serde(default)]
    pub restart: bool,
    /// Startup timeout in milliseconds. Default: 10000.
    #[serde(default = "default_connector_timeout")]
    pub timeout: u64,
}

impl Default for ConnectorProcessConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            config: None,
            auto_pair: true,
            restart: false,
            timeout: 10000,
        }
    }
}

fn bool_true() -> bool {
    true
}

fn default_connector_timeout() -> u64 {
    10_000
}

// ---- Sub-Agents ----

/// User-configured sub-agents, each registered as a callable tool (`agent_{name}`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubAgentsConfig(pub HashMap<String, SubAgentConfig>);

/// Configuration for a single sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    /// LLM provider name (must exist in `models.providers`). Defaults to `models.default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Custom persona injected as system prompt.
    pub persona: String,
    /// Tool description shown to the parent agent.
    pub description: String,
    /// Allowed tool names for this sub-agent (empty = all registered tools minus other agents).
    #[serde(default)]
    pub tools: Vec<String>,
    /// How much conversation context the sub-agent receives.
    #[serde(default)]
    pub context_mode: ContextMode,
    /// Optional turn budget overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetConfig>,
}

/// Controls how much context a sub-agent receives from the parent conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMode {
    /// Sub-agent receives only the task + explicit context. Default.
    #[default]
    TaskOnly,
    /// Sub-agent receives the conversation history (without user profile/memories).
    Conversation,
}

// ---- Duration serde helper ----

mod option_duration_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => {
                let s = format!("{}ms", d.as_millis());
                serializer.serialize_str(&s)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(s) => parse_duration(&s)
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }

    fn parse_duration(s: &str) -> Result<Duration, String> {
        if let Some(ms) = s.strip_suffix("ms") {
            let n: u64 = ms.parse().map_err(|e| format!("invalid duration: {e}"))?;
            return Ok(Duration::from_millis(n));
        }
        if let Some(secs) = s.strip_suffix('s') {
            let n: f64 = secs.parse().map_err(|e| format!("invalid duration: {e}"))?;
            return Ok(Duration::from_secs_f64(n));
        }
        if let Some(mins) = s.strip_suffix('m') {
            let n: f64 = mins.parse().map_err(|e| format!("invalid duration: {e}"))?;
            return Ok(Duration::from_secs_f64(n * 60.0));
        }
        Err(format!("unsupported duration format: {s}"))
    }
}
