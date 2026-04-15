use std::sync::Arc;
use std::time::Duration;

use ozzie_core::actors::{ActorPool, ActorPoolConfig};
use ozzie_core::conscience::ToolPermissions;
use ozzie_core::domain::Tool;
use ozzie_core::events::{Bus, EventBus};
use ozzie_core::prompt;
use ozzie_llm::providers::{OllamaProvider, OpenAIProvider};
use ozzie_llm::{AuthKind, Provider, ResolvedAuth};
use ozzie_protocol::EventKind;
use ozzie_runtime::{EventRunner, EventRunnerConfig, FileSessionStore};
use tempfile::TempDir;
use tokio::net::TcpListener;

use ozzie_gateway::handler::RequestHandler;
use ozzie_gateway::{AppState, Hub, HubHandler, Server, ServerConfig};

// ---- Provider detection ----
// Priority: OPENAI_API_KEY → OLLAMA_URL → localhost ollama → skip

/// Tries to build a provider from env vars.
/// 1. OPENAI_API_KEY + OPENAI_BASE_URL → OpenAI-compatible (GitHub Models, etc.)
/// 2. OLLAMA_URL (or localhost) → Ollama
pub async fn probe_provider() -> Option<Arc<dyn Provider>> {
    // OpenAI-compatible (GitHub Models, custom endpoints)
    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        let base_url = std::env::var("OPENAI_BASE_URL").ok();
        return Some(Arc::new(OpenAIProvider::new(
            ResolvedAuth { kind: AuthKind::ApiKey, value: api_key },
            Some(&model),
            base_url.as_deref(),
            None,
            None,
            None,
        )));
    }

    // Ollama (explicit or localhost)
    let url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen3:8b".into());
    let resp = reqwest::get(format!("{url}/api/tags")).await.ok()?;
    if resp.status().is_success() {
        Some(Arc::new(OllamaProvider::new(&model, Some(&url), None)))
    } else {
        None
    }
}

/// Tries to build a vision-capable provider.
/// Same priority as `probe_provider` but uses vision-specific model env vars.
pub async fn probe_vision_provider() -> Option<Arc<dyn Provider>> {
    // OpenAI-compatible — vision models support images natively
    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        let model = std::env::var("OPENAI_VISION_MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".into());
        let base_url = std::env::var("OPENAI_BASE_URL").ok();
        return Some(Arc::new(OpenAIProvider::new(
            ResolvedAuth { kind: AuthKind::ApiKey, value: api_key },
            Some(&model),
            base_url.as_deref(),
            None,
            None,
            None,
        )));
    }

    // Ollama with vision model
    let url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into());
    let model = std::env::var("OLLAMA_VISION_MODEL")
        .or_else(|_| std::env::var("OLLAMA_MODEL"))
        .unwrap_or_else(|_| "gemma3:4b".into());
    let resp = reqwest::get(format!("{url}/api/tags")).await.ok()?;
    if resp.status().is_success() {
        Some(Arc::new(OllamaProvider::new(&model, Some(&url), None)))
    } else {
        None
    }
}

macro_rules! require_provider {
    ($name:ident) => {
        let $name = match crate::harness::probe_provider().await {
            Some(v) => v,
            None => {
                eprintln!("SKIP: no LLM provider available (set OPENAI_API_KEY or OLLAMA_URL)");
                return;
            }
        };
    };
}

macro_rules! require_vision_provider {
    ($name:ident) => {
        let $name = match crate::harness::probe_vision_provider().await {
            Some(v) => v,
            None => {
                eprintln!("SKIP: no vision provider available (set OPENAI_API_KEY or OLLAMA_VISION_MODEL)");
                return;
            }
        };
    };
}

// ---- NoopHandler placeholder ----

struct NoopHandler;

#[async_trait::async_trait]
impl HubHandler for NoopHandler {
    async fn handle_request(
        &self,
        _client_id: u64,
        frame: ozzie_gateway::Frame,
    ) -> ozzie_gateway::Frame {
        ozzie_gateway::Frame::response_err(
            frame.id.unwrap_or_default(),
            -32603,
            "handler not ready",
        )
    }
}

// ---- TestGateway ----

#[allow(dead_code)]
pub struct TestGateway {
    pub port: u16,
    pub bus: Arc<dyn EventBus>,
    pub sessions: Arc<FileSessionStore>,
    pub tempdir: TempDir,
}

pub struct TestGatewayConfig {
    pub provider: Arc<dyn Provider>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub blob_store: Option<Arc<dyn ozzie_core::domain::BlobStore>>,
}

impl TestGateway {
    pub async fn start(config: TestGatewayConfig) -> Self {
        let tempdir = TempDir::new().expect("create tempdir");
        let sessions_dir = tempdir.path().join("sessions");
        let sessions = Arc::new(
            FileSessionStore::new(&sessions_dir).expect("create session store"),
        );
        let bus = Arc::new(Bus::new(256));

        let provider_name = config.provider.name().to_string();
        let permissions = Arc::new(ToolPermissions::new(Vec::new()));

        // ActorPool with 1 slot
        let mut actor_cfg = ActorPoolConfig::default();
        actor_cfg
            .actors_per_provider
            .insert(provider_name.clone(), 1);
        let pool = Arc::new(ActorPool::new(actor_cfg));

        let blob_store_for_runner = config.blob_store.clone();

        // EventRunner — minimal config, no dangerous tool wrapping
        let runner = Arc::new(EventRunner::with_config(EventRunnerConfig {
            bus: bus.clone(),
            sessions: sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
            provider: config.provider,
            persona: prompt::DEFAULT_PERSONA.to_string(),
            agent_instructions: prompt::AGENT_INSTRUCTIONS.to_string(),
            preferred_language: None,
            skill_descriptions: Default::default(),
            custom_instructions: None,
            tools: config.tools,
            retriever: None,
            compressor: None,
            permissions: None,
            approver: None,
            dangerous_tool_names: Vec::new(),
            pairing_manager: None,
            actor_infos: Vec::new(),
            pool: Some(pool),
            provider_name: Some(provider_name),
            context_window: None,
            user_profile: None,
            blob_store: blob_store_for_runner,
            project_registry: None,
        }));
        runner.start();

        // Hub + RequestHandler
        let placeholder: Arc<dyn HubHandler> = Arc::new(NoopHandler);
        let hub = Hub::new(bus.clone(), placeholder);
        let mut handler = RequestHandler::new(
            bus.clone(),
            sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
            hub.clone(),
        )
        .with_permissions(permissions);
        if let Some(store) = config.blob_store {
            handler = handler.with_blob_store(store);
        }
        hub.set_handler(Arc::new(handler));

        // Server on free port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind free port");
        let port = listener.local_addr().expect("local addr").port();

        let state = AppState {
            hub,
            bus: bus.clone() as Arc<dyn EventBus>,
            authenticator: None,
            sessions: Some(sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>),
            pairing_manager: None,
            chat_storage: None,
            device_storage: None,
            device_approvals: None,
            local_key: None,
            memory_store: None,
            page_store: None,
            ozzie_path: std::path::PathBuf::new(),
        };

        let server = Server::new(ServerConfig::default(), state);
        let router = server.router();
        tokio::spawn(async move {
            axum::serve(listener, router).await.ok();
        });

        // Brief pause for server readiness
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            port,
            bus,
            sessions,
            tempdir,
        }
    }

    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}/api/ws", self.port)
    }

    pub async fn connect(&self) -> ozzie_client::OzzieClient {
        ozzie_client::OzzieClient::connect(&self.ws_url(), None)
            .await
            .expect("connect to test gateway")
    }

    pub fn work_dir(&self) -> std::path::PathBuf {
        self.tempdir.path().to_path_buf()
    }
}

// ---- Frame collection helpers ----

/// Reads frames until we see an `assistant.message` event or timeout.
/// Returns all collected frames.
pub async fn collect_until_assistant_message(
    client: &mut ozzie_client::OzzieClient,
    timeout: Duration,
) -> Vec<ozzie_gateway::Frame> {
    let mut frames = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, client.read_frame()).await {
            Ok(Ok(frame)) => {
                let is_done = frame.event_kind() == Some(EventKind::AssistantMessage);
                frames.push(frame);
                if is_done {
                    break;
                }
            }
            Ok(Err(e)) => {
                eprintln!("frame read error: {e}");
                break;
            }
            Err(_) => break, // timeout
        }
    }

    frames
}

/// Extracts the text content from an `assistant.message` frame.
pub fn extract_assistant_text(frames: &[ozzie_gateway::Frame]) -> Option<String> {
    frames
        .iter()
        .filter(|f| f.event_kind() == Some(EventKind::AssistantMessage))
        .find_map(|f| {
            f.params
                .as_ref()
                .and_then(|p| p.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// Counts frames matching a given event kind.
pub fn count_events(frames: &[ozzie_gateway::Frame], kind: EventKind) -> usize {
    frames
        .iter()
        .filter(|f| f.event_kind() == Some(kind))
        .count()
}
