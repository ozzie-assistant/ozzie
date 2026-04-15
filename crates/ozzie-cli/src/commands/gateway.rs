use std::collections::HashMap;
use std::sync::Arc;

use clap::Args;
use tracing::{error, info, warn};

// Connectors are now standalone JSON-RPC bridges:
//   ozzie connector discord start
//   ozzie connector file start
use ozzie_discord_bridge::DiscordDatabase;
use ozzie_core::actors::{ActorPool, ActorPoolConfig};
use ozzie_core::config;
use ozzie_utils::config::{
    config_path, discord_db_path, dotenv_path, logs_path, memory_path, ozzie_path, sessions_path,
    skills_path,
};
use ozzie_core::conscience::ToolPermissions;
use ozzie_core::domain::{Message, PairingStorage, SubAgentRunner, SubtaskRunner, ToolError, ToolSet, TOOL_CTX};
use ozzie_core::events::Bus;
use ozzie_core::policy::MemoryPendingPairings;
use ozzie_runtime::JsonPairingStore;
use ozzie_core::prompt;
use ozzie_utils::secrets;
use ozzie_core::skills::{self, SkillRegistry};
use ozzie_core::storage::ConfigStore;
use ozzie_runtime::storage::FileStorage;
use ozzie_gateway::hub::HubHandler;
use ozzie_gateway::{AppState, Hub, Server, ServerConfig};
use ozzie_memory::{MarkdownPageStore, MarkdownStore};
use ozzie_runtime::approval::EventBusApprovalRequester;
use ozzie_runtime::scheduler::Scheduler;
use ozzie_runtime::{
    CostTracker, EventRunner, EventRunnerConfig, FileSessionStore,
    LayeredContextCompressor, PairingManager, ProcessSupervisor, ProviderRegistry, ReactConfig, ReactLoop, ReactResult, TurnBudget,
};
use ozzie_tools::native;
use ozzie_tools::ToolRegistry;

/// Arguments for the gateway command.
#[derive(Args)]
pub struct GatewayArgs {
    /// Listen host.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Listen port.
    #[arg(long, default_value_t = 18420)]
    port: u16,

    /// Disable authentication (dev mode).
    #[arg(long)]
    insecure: bool,

    /// Run as a background daemon (delegates to `ozzie daemon start`).
    #[arg(long)]
    daemon: bool,
}

/// Starts the gateway server with all infrastructure wired.
pub async fn run(args: GatewayArgs, _config_path: Option<&str>) -> anyhow::Result<()> {
    if args.daemon {
        return super::daemon::run(super::daemon::DaemonArgs::start(args.port)).await;
    }

    info!("initializing gateway");

    load_dotenv_with_decrypt()?;
    let cfg = load_config();

    let bus = Arc::new(Bus::new(cfg.events.buffer_size));

    let sessions = init_session_store()?;
    let (memory_store, page_store) = init_memory().await?;

    let (provider_registry, actor_pool) = create_providers(&cfg)?;
    let provider_registry = Arc::new(provider_registry);
    let actor_pool = Arc::new(actor_pool);
    let provider = provider_registry.default_provider().clone();
    info!(
        default = provider_registry.default_name(),
        providers = ?provider_registry.names(),
        "LLM providers initialized"
    );

    let default_context_window = cfg
        .models
        .providers
        .get(&cfg.models.default)
        .and_then(|p| p.context_window);
    let compressor = init_compressor(&cfg, provider.clone(), default_context_window);

    let persona = prompt::load_persona(&ozzie_path());
    info!(chars = persona.len(), "persona loaded");

    let user_profile = ozzie_core::profile::load(&ozzie_path())
        .ok()
        .flatten();
    if user_profile.is_some() {
        info!("user profile loaded");
    }

    let (skill_registry, skill_descs) = init_skills();

    let (tool_registry, mcp_shutdown) = init_tools(
        &cfg,
        bus.clone(),
        sessions.clone(),
        memory_store.clone(),
        skill_registry.clone(),
        provider_registry.clone(),
        actor_pool.clone(),
    )
    .await?;

    // Project workspaces
    let project_registry = init_projects(&cfg);
    native::register_project_tools(
        &tool_registry,
        project_registry.clone(),
        skill_registry.clone(),
        sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
        resolve_workspaces_root(&cfg),
    );
    native::register_create_skill_tool(
        &tool_registry,
        skill_registry,
        project_registry.clone(),
        sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
        skills_path(),
    );
    info!(count = project_registry.len(), "projects discovered");

    let discord_db: Arc<dyn ConfigStore<DiscordDatabase>> =
        Arc::new(FileStorage::new(discord_db_path()));

    let (pairing_manager, chat_storage) = init_pairing(bus.clone(), &discord_db)?;

    let permissions = Arc::new(ToolPermissions::new(Vec::new()));
    let approver = Arc::new(EventBusApprovalRequester::new(bus.clone()));

    let dangerous_tool_names: Vec<String> = tool_registry
        .names()
        .into_iter()
        .filter(|name| tool_registry.is_dangerous(name))
        .collect();
    info!(count = dangerous_tool_names.len(), "dangerous tools identified");

    // Sub-agent tools — registered after permissions/approver are available.
    if !cfg.sub_agents.0.is_empty() {
        let sub_agent_runner: Arc<dyn SubAgentRunner> = Arc::new(DirectSubAgentRunner {
            provider_registry: provider_registry.clone(),
            tool_registry: tool_registry.clone(),
            permissions: permissions.clone(),
            approver: approver.clone() as Arc<dyn ozzie_core::conscience::ApprovalRequester>,
            bus: bus.clone(),
            dangerous_tool_names: dangerous_tool_names.clone(),
            sessions: sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
        });
        native::register_sub_agent_tools(&tool_registry, &cfg.sub_agents, sub_agent_runner);
        info!(count = cfg.sub_agents.0.len(), "sub-agent tools registered");
    }

    let tools = tool_registry.all_tools();
    info!(count = tools.len(), "total tools registered");

    let actor_infos = actor_pool.available_actors();
    let runner = Arc::new(EventRunner::with_config(EventRunnerConfig {
        bus: bus.clone(),
        sessions: sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
        provider,
        persona,
        agent_instructions: prompt::AGENT_INSTRUCTIONS.to_string(),
        preferred_language: cfg.agent.preferred_language.clone(),
        skill_descriptions: skill_descs,
        custom_instructions: cfg.agent.system_prompt.clone(),
        tools,
        retriever: Some({
            let fts = Arc::new(ozzie_runtime::memory_retriever::FtsMemoryRetriever::new(
                memory_store.clone() as Arc<dyn ozzie_core::domain::MemoryStore>,
            ));
            Arc::new(ozzie_runtime::page_retriever::PageAwareRetriever::new(
                page_store.clone() as Arc<dyn ozzie_core::domain::PageStore>,
                fts as Arc<dyn ozzie_core::domain::MemoryRetriever>,
            ))
        }),
        compressor,
        permissions: Some(permissions.clone()),
        approver: Some(approver as Arc<dyn ozzie_core::conscience::ApprovalRequester>),
        dangerous_tool_names,
        pairing_manager: Some(pairing_manager.clone()),
        actor_infos,
        pool: Some(actor_pool.clone()),
        provider_name: Some(cfg.models.default.clone()),
        context_window: cfg
            .models
            .providers
            .get(&cfg.models.default)
            .and_then(|p| p.context_window),
        user_profile,
        blob_store: Some(Arc::new(ozzie_runtime::FsBlobStore::new(ozzie_path()))),
        project_registry: Some(project_registry.clone()),
    }));
    runner.start();

    let _event_logger =
        ozzie_runtime::event_logger::EventLogger::start(logs_path(), bus.clone());
    info!(logs_dir = %logs_path().display(), "event logger started");

    let _heartbeat = ozzie_runtime::heartbeat::Writer::new(ozzie_path().join("heartbeat.json"));
    _heartbeat.start().await;
    info!("heartbeat started");

    let _cost_tracker = CostTracker::new(
        bus.clone(),
        sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
    );
    info!("cost tracker started");

    // Dream consolidation — extracts lasting knowledge from conversations.
    let dream_runner = Arc::new(
        ozzie_runtime::DreamRunner::new(
            sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>,
            memory_store.clone() as Arc<dyn ozzie_memory::Store>,
            provider_registry.default_provider().clone(),
            &ozzie_path(),
            bus.clone(),
        )
        .with_page_store(page_store.clone() as Arc<dyn ozzie_core::domain::PageStore>)
        .with_project_registry(project_registry.clone()),
    );
    dream_runner.start().await;
    info!("dream consolidation started (12h interval)");

    let device_storage = Arc::new(ozzie_runtime::JsonDeviceStore::new(&ozzie_path()));
    let device_approvals = Arc::new(ozzie_gateway::DeviceApprovalCache::new());
    let authenticator = init_auth(&args, device_storage.clone())?;

    // Connector process supervisor — spawns bridges as child processes.
    let supervisor = init_connector_supervisor(&cfg, &args).await;
    supervisor.start_all().await;
    let _monitor = supervisor.start_monitor();
    let _ = discord_db; // kept for pairing DB reference

    setup_file_logging()?;

    let local_key = ozzie_client::OzzieClient::read_or_generate_key(&ozzie_path());
    info!(key = %local_key, "gateway device key loaded");

    let hub = init_hub(bus.clone(), sessions.clone(), permissions);

    let state = AppState {
        hub,
        bus: bus as Arc<dyn ozzie_core::events::EventBus>,
        authenticator,
        sessions: Some(sessions.clone() as Arc<dyn ozzie_runtime::SessionStore>),
        pairing_manager: Some(pairing_manager),
        chat_storage: Some(chat_storage as Arc<dyn PairingStorage>),
        device_storage: Some(device_storage as Arc<dyn ozzie_core::domain::DeviceStorage>),
        device_approvals: Some(device_approvals),
        local_key: Some(local_key),
        memory_store: Some(memory_store.clone() as Arc<dyn ozzie_core::domain::MemoryStore>),
        page_store: Some(page_store.clone() as Arc<dyn ozzie_core::domain::PageStore>),
        ozzie_path: ozzie_path(),
    };

    let server = Server::new(ServerConfig { host: args.host, port: args.port }, state);
    info!("starting gateway — press Ctrl+C to stop");

    tokio::select! {
        result = server.serve() => {
            dream_runner.stop().await;
            mcp_shutdown.shutdown_all().await;
            supervisor.stop_all().await;
            result.map_err(|e| anyhow::anyhow!(e))
        }
        _ = tokio::signal::ctrl_c() => {
            info!("shutting down");
            dream_runner.stop().await;
            mcp_shutdown.shutdown_all().await;
            supervisor.stop_all().await;
            Ok(())
        }
    }
}

// ---- Infrastructure init ----

fn load_config() -> config::Config {
    let config_path = config_path();
    if config_path.exists() {
        let store = secrets::SecretStore::global();
        let opts = config::LoadOptions {
            resolver: Some(Box::new(move |var_name: &str| {
                store.resolve_template_var(var_name)
            })),
            ..Default::default()
        };
        config::load_with_options(&config_path, opts).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load config, using defaults");
            config::Config::default()
        })
    } else {
        tracing::warn!("no config file found, using defaults");
        config::Config::default()
    }
}

fn init_session_store() -> anyhow::Result<Arc<FileSessionStore>> {
    let sessions = Arc::new(
        FileSessionStore::new(&sessions_path())
            .map_err(|e| anyhow::anyhow!("init session store: {e}"))?,
    );
    info!(sessions_dir = %sessions_path().display(), "session store initialized");
    Ok(sessions)
}

async fn init_memory() -> anyhow::Result<(Arc<MarkdownStore>, Arc<MarkdownPageStore>)> {
    let store = Arc::new(
        MarkdownStore::new(&memory_path())
            .map_err(|e| anyhow::anyhow!("init memory store: {e}"))?,
    );

    // Migrate existing SQLite-only entries to markdown files (one-time)
    let migrated = store
        .migrate_from_sqlite()
        .await
        .map_err(|e| anyhow::anyhow!("memory migration: {e}"))?;
    if migrated > 0 {
        info!(count = migrated, "migrated legacy memories to markdown");
    }

    // Rebuild index from markdown files (SsoT)
    let indexed = store
        .rebuild_index()
        .map_err(|e| anyhow::anyhow!("rebuild memory index: {e}"))?;
    info!(
        entries = indexed,
        memory_dir = %memory_path().display(),
        "memory store initialized (markdown SsoT)"
    );

    // Wiki page store — shares the same database, pages in a subdirectory
    let pages_dir = memory_path().join("pages");
    let db_path = memory_path().join(".cache").join("memory.db");
    let page_store = Arc::new(
        MarkdownPageStore::new(&pages_dir, &db_path)
            .map_err(|e| anyhow::anyhow!("init page store: {e}"))?,
    );
    let page_count = page_store
        .rebuild_index()
        .map_err(|e| anyhow::anyhow!("rebuild page index: {e}"))?;
    info!(pages = page_count, "wiki page store initialized");

    Ok((store, page_store))
}

fn init_compressor(
    cfg: &config::Config,
    provider: Arc<dyn ozzie_llm::Provider>,
    context_window: Option<usize>,
) -> Option<Arc<dyn ozzie_core::domain::ContextCompressor>> {
    if !cfg.layered_context.is_enabled() {
        return None;
    }
    let max_prompt_tokens = context_window.unwrap_or(100_000);
    let layered_cfg = ozzie_core::layered::Config {
        max_archives: cfg.layered_context.max_archives,
        max_recent_messages: cfg.layered_context.max_recent_messages,
        archive_chunk_size: cfg.layered_context.archive_chunk_size,
        max_prompt_tokens,
        ..ozzie_core::layered::Config::default()
    };

    // Use LLM-backed summarizer with the default provider
    let summarizer = ozzie_runtime::llm_summarizer::llm_summarizer(provider);
    let store = Box::new(ozzie_runtime::layered_store::FileArchiveStore::new(sessions_path()));
    let manager = ozzie_core::layered::Manager::new(store, layered_cfg.clone(), summarizer);

    info!(
        max_recent = cfg.layered_context.max_recent_messages,
        max_archives = cfg.layered_context.max_archives,
        "context compression enabled (LLM-backed)"
    );
    Some(Arc::new(LayeredContextCompressor::from_manager(manager)))
}

fn init_skills() -> (Arc<SkillRegistry>, std::collections::HashMap<String, String>) {
    let skills_dir = skills_path();
    let loaded = skills::load_skills_dir(&skills_dir);
    let descs = skills::skill_descriptions(&loaded);
    let registry = Arc::new(SkillRegistry::new());
    for skill in loaded {
        registry.register(skill);
    }
    info!(count = registry.len(), "skills loaded");
    (registry, descs)
}

// ---- Tool registry ----

async fn init_tools(
    cfg: &config::Config,
    bus: Arc<Bus>,
    sessions: Arc<FileSessionStore>,
    memory_store: Arc<MarkdownStore>,
    skill_registry: Arc<SkillRegistry>,
    provider_registry: Arc<ProviderRegistry>,
    actor_pool: Arc<ActorPool>,
) -> anyhow::Result<(Arc<ToolRegistry>, ozzie_tools::mcp::McpShutdownHandle)> {
    let registry = Arc::new(ToolRegistry::new());

    let command_sandbox: Arc<dyn ozzie_core::domain::CommandSandbox> =
        Arc::from(ozzie_runtime::sandbox::create_command_sandbox());
    info!(backend = command_sandbox.backend_name(), "OS sandbox initialized");
    native::register_all(&registry, Some(command_sandbox));
    native::register_memory_tools(&registry, memory_store, None);
    native::register_session_tools(
        &registry,
        sessions as Arc<dyn ozzie_runtime::SessionStore>,
    );

    let names = registry.names();
    let core_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let tool_set = Arc::new(ToolSet::new(&core_refs, &core_refs));
    native::register_activate_tool(&registry, tool_set, registry.clone(), Some(skill_registry));

    // Subtask runner — uses the pool + registry to resolve tools and provider at invocation time.
    let subtask_runner: Arc<dyn SubtaskRunner> = Arc::new(DirectSubtaskRunner {
        provider_registry: provider_registry.clone(),
        pool: actor_pool.clone(),
        tool_registry: registry.clone(),
    });
    native::register_subtask_tool(&registry, subtask_runner);

    // Scheduler — runs ReAct loop directly on trigger.
    let bus_for_scheduler = bus.clone();
    let bus_for_schedule_tools = bus.clone();
    let sched_handler = Arc::new(DirectScheduleHandler {
        provider_registry,
        pool: actor_pool,
        tool_registry: registry.clone(),
    });
    let scheduler = Arc::new(Scheduler::new(bus_for_scheduler, sched_handler));
    native::register_schedule_tools(&registry, scheduler.clone(), bus_for_schedule_tools);
    let _sched_handle = scheduler.start();
    info!("scheduler started");

    let (mcp_results, mcp_shutdown) = ozzie_tools::mcp::setup_mcp_servers(&cfg.mcp, &registry).await;
    let mcp_tool_count: usize = mcp_results.iter().map(|r| r.registered_tools.len()).sum();
    info!(
        servers = mcp_results.len(),
        tools = mcp_tool_count,
        "MCP servers initialized"
    );

    Ok((registry, mcp_shutdown))
}

// ---- Pairing ----

fn init_pairing(
    bus: Arc<Bus>,
    discord_db: &Arc<dyn ConfigStore<DiscordDatabase>>,
) -> anyhow::Result<(Arc<PairingManager>, Arc<JsonPairingStore>)> {
    let chat_storage = Arc::new(JsonPairingStore::new(&ozzie_path()));
    let pending_pairings = Arc::new(MemoryPendingPairings::new());

    let guild_role_policies = discord_db
        .read()
        .map(|db| {
            db.guilds
                .into_iter()
                .filter(|(_, g)| !g.role_policies.is_empty())
                .map(|(guild_id, g)| (guild_id, g.role_policies))
                .collect()
        })
        .unwrap_or_default();

    let pairing_manager = Arc::new(PairingManager::new_with_guild_roles(
        pending_pairings,
        chat_storage.clone() as Arc<dyn PairingStorage>,
        bus,
        guild_role_policies,
    ));
    Ok((pairing_manager, chat_storage))
}

// ---- Hub ----

fn init_hub(
    bus: Arc<Bus>,
    sessions: Arc<FileSessionStore>,
    permissions: Arc<ToolPermissions>,
) -> Arc<Hub> {
    let placeholder = Arc::new(NoopHandler);
    let hub = Hub::new(bus.clone(), placeholder as Arc<dyn HubHandler>);

    let handler = Arc::new(
        ozzie_gateway::handler::RequestHandler::new(
            bus,
            sessions as Arc<dyn ozzie_runtime::SessionStore>,
            hub.clone(),
        )
        .with_permissions(permissions),
    );
    hub.set_handler(handler);
    hub
}

// ---- Auth ----

fn init_auth(
    args: &GatewayArgs,
    device_storage: Arc<ozzie_runtime::JsonDeviceStore>,
) -> anyhow::Result<Option<Arc<dyn ozzie_core::auth::Authenticator>>> {
    if args.insecure {
        info!("auth disabled (insecure mode)");
        return Ok(None);
    }

    let token_path = ozzie_path().join(".token");
    let token = match std::fs::read_to_string(&token_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(t) => {
            info!(path = %token_path.display(), "loaded auth token");
            t
        }
        None => {
            let auth = ozzie_core::auth::LocalAuth::generate();
            let t = auth.token().to_string();
            if let Err(e) = std::fs::write(&token_path, &t) {
                tracing::warn!(error = %e, "failed to persist .token");
            } else {
                info!(path = %token_path.display(), "generated auth token");
            }
            t
        }
    };

    let local_auth = Arc::new(ozzie_core::auth::LocalAuth::new(token))
        as Arc<dyn ozzie_core::auth::Authenticator>;
    let device_auth = Arc::new(ozzie_core::auth::DeviceAuth::new(
        device_storage as Arc<dyn ozzie_core::domain::DeviceStorage>,
    )) as Arc<dyn ozzie_core::auth::Authenticator>;

    Ok(Some(Arc::new(ozzie_core::auth::CompositeAuth::new(vec![
        local_auth,
        device_auth,
    ])) as Arc<dyn ozzie_core::auth::Authenticator>))
}

// ---- Helpers ----

fn load_dotenv_with_decrypt() -> anyhow::Result<()> {
    let dotenv_path = dotenv_path();
    if !dotenv_path.exists() {
        return Ok(());
    }

    let entries =
        secrets::load_dotenv(&dotenv_path).map_err(|e| anyhow::anyhow!("load dotenv: {e}"))?;
    if entries.is_empty() {
        return Ok(());
    }

    let enc_svc = crate::crypt::AgeEncryptionService::new(&ozzie_path());

    let store = secrets::SecretStore::global();

    for (key, value) in &entries {
        let resolved = if secrets::is_encrypted(value) {
            if enc_svc.is_available() {
                match enc_svc.decrypt(value) {
                    Ok(plain) => plain,
                    Err(e) => {
                        tracing::warn!(key, error = %e, "failed to decrypt, skipping");
                        continue;
                    }
                }
            } else {
                tracing::warn!(key, "encrypted value but no age key found, skipping");
                continue;
            }
        } else {
            value.clone()
        };

        // Store in SecretStore (in-memory) — NOT in std::env.
        // This prevents secrets from being exposed via printenv or /proc/self/environ.
        store.set(key, &resolved);
        tracing::debug!(key, "secret stored (in-memory)");
    }

    info!(count = entries.len(), "dotenv loaded into secret store");
    Ok(())
}

async fn init_connector_supervisor(
    cfg: &config::Config,
    args: &GatewayArgs,
) -> std::sync::Arc<ProcessSupervisor> {
    let gateway_url = format!("ws://{}:{}/api/ws", args.host, args.port);
    let supervisor = std::sync::Arc::new(ProcessSupervisor::new(gateway_url, ozzie_path()));

    let secret_store = secrets::SecretStore::global();

    for (name, config) in &cfg.connectors.0 {
        let mut resolved = config.clone();
        // Resolve ${{ .Env.* }} templates in env values
        for value in resolved.env.values_mut() {
            if let Some(resolved_val) = resolve_env_template(value, secret_store) {
                *value = resolved_val;
            }
        }
        supervisor.register(name.clone(), resolved).await;
    }

    if !cfg.connectors.0.is_empty() {
        info!(count = cfg.connectors.0.len(), "connector supervisor initialized");
    }

    supervisor
}

/// Resolves `${{ .Env.VAR }}` template to the secret store value.
fn resolve_env_template(value: &str, store: &secrets::SecretStore) -> Option<String> {
    if !value.starts_with("${{ .Env.") || !value.ends_with(" }}") {
        return None;
    }
    let var_name = &value[9..value.len() - 3];
    store.get(var_name)
}

fn setup_file_logging() -> anyhow::Result<()> {
    use std::io::Write;
    let log_path = logs_path().join("gateway.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(f, "--- gateway started at {} ---", chrono::Utc::now())?;
    info!(log_file = %log_path.display(), "file logging initialized");
    Ok(())
}

fn build_provider(
    name: &str,
    provider_cfg: &config::ProviderConfig,
) -> anyhow::Result<Arc<dyn ozzie_llm::Provider>> {
    crate::provider_factory::build_provider(name, provider_cfg)
}

fn create_providers(cfg: &config::Config) -> anyhow::Result<(ProviderRegistry, ActorPool)> {
    let default_name = &cfg.models.default;
    let mut registry = ProviderRegistry::new(default_name.clone());

    let mut actors_per_provider = HashMap::new();
    let mut tags_per_provider = HashMap::new();
    let mut capabilities_per_provider = HashMap::new();

    for (name, provider_cfg) in &cfg.models.providers {
        let provider = build_provider(name, provider_cfg)?;
        info!(
            provider = name,
            driver = %provider_cfg.driver,
            model = %provider_cfg.model,
            max_concurrent = provider_cfg.max_concurrent,
            "LLM provider initialized"
        );
        registry.register(name.clone(), provider);

        // Register per-provider turn budget if configured
        if let Some(ref budget_cfg) = provider_cfg.budget {
            let budget = TurnBudget::default().with_config_overrides(budget_cfg);
            info!(
                provider = name,
                max_turns = budget.max_turns,
                max_output_tokens = budget.max_output_tokens,
                timeout_sec = budget.timeout.as_secs(),
                "custom turn budget"
            );
            registry.set_budget(name.clone(), budget);
        }

        actors_per_provider.insert(name.clone(), provider_cfg.max_concurrent as usize);
        if !provider_cfg.tags.is_empty() {
            tags_per_provider.insert(name.clone(), provider_cfg.tags.clone());
        }
        if !provider_cfg.capabilities.is_empty() {
            capabilities_per_provider.insert(name.clone(), provider_cfg.capabilities.clone());
        }
    }

    // Ensure the default provider is configured
    if registry.get(default_name).is_none() {
        anyhow::bail!("default provider '{default_name}' not found in providers config");
    }

    // Wire fallback chains: if a provider has `fallback = "other"`, wrap it
    // with FallbackProvider so failures automatically route to the fallback.
    let fallback_pairs: Vec<(String, String)> = cfg
        .models
        .providers
        .iter()
        .filter_map(|(name, pcfg)| {
            pcfg.fallback
                .as_ref()
                .map(|fb| (name.clone(), fb.clone()))
        })
        .collect();

    for (name, fallback_name) in fallback_pairs {
        let primary = match registry.get(&name) {
            Some(p) => p.clone(),
            None => continue,
        };
        let fallback = match registry.get(&fallback_name) {
            Some(f) => f.clone(),
            None => {
                warn!(
                    provider = %name,
                    fallback = %fallback_name,
                    "fallback provider not found, skipping"
                );
                continue;
            }
        };
        info!(
            provider = %name,
            fallback = %fallback_name,
            "fallback chain configured"
        );
        let wrapped = Arc::new(ozzie_llm::FallbackProvider::new(primary, fallback));
        registry.register(name, wrapped);
    }

    let pool = ActorPool::new(ActorPoolConfig {
        actors_per_provider,
        tags_per_provider,
        capabilities_per_provider,
        ..ActorPoolConfig::default()
    });

    Ok((registry, pool))
}

// ---- Placeholder handlers ----

struct NoopHandler;

#[async_trait::async_trait]
impl HubHandler for NoopHandler {
    async fn handle_request(
        &self,
        _client_id: u64,
        frame: ozzie_gateway::Frame,
    ) -> ozzie_gateway::Frame {
        ozzie_gateway::Frame::response_err(frame.id.unwrap_or_default(), -32603, "handler not ready")
    }
}

/// Inline subtask runner — executes a ReAct loop directly.
/// Uses the ActorPool for capacity management and ProviderRegistry for provider lookup.
struct DirectSubtaskRunner {
    provider_registry: Arc<ProviderRegistry>,
    pool: Arc<ActorPool>,
    tool_registry: Arc<ToolRegistry>,
}

#[async_trait::async_trait]
impl SubtaskRunner for DirectSubtaskRunner {
    async fn run_subtask(
        &self,
        instruction: &str,
        tools: &[String],
        work_dir: Option<&str>,
        subtask_depth: u32,
        provider: Option<&str>,
        tags: &[String],
    ) -> Result<String, ToolError> {
        // Resolve provider: explicit name > tag-based lookup > default
        let provider_name = if let Some(name) = provider {
            name.to_string()
        } else if !tags.is_empty() {
            // Find an idle provider matching the requested tags
            self.pool
                .find_idle_by_tags(tags)
                .unwrap_or_else(|| self.provider_registry.default_name().to_string())
        } else {
            self.provider_registry.default_name().to_string()
        };

        // Inline subtasks reuse the parent's actor slot — no acquire needed.
        // The parent ReactLoop already holds a slot; acquiring another would
        // deadlock when max_concurrent == 1 (non-parallelizable model).
        let llm_provider = self
            .provider_registry
            .get(&provider_name)
            .ok_or_else(|| {
                ToolError::Execution(format!("unknown provider: {provider_name}"))
            })?
            .clone();

        self.run_subtask_inner(instruction, tools, work_dir, subtask_depth, &llm_provider, &provider_name)
            .await
    }
}

impl DirectSubtaskRunner {
    async fn run_subtask_inner(
        &self,
        instruction: &str,
        tools: &[String],
        work_dir: Option<&str>,
        _subtask_depth: u32,
        provider: &Arc<dyn ozzie_llm::Provider>,
        provider_name: &str,
    ) -> Result<String, ToolError> {
        let all_tools = self.tool_registry.all_tools();
        let resolved_tools: Vec<_> = if tools.is_empty() {
            all_tools
        } else {
            let allowed: std::collections::HashSet<&str> =
                tools.iter().map(|s| s.as_str()).collect();
            all_tools
                .into_iter()
                .filter(|t| allowed.contains(t.info().name.as_str()))
                .collect()
        };

        let mut full_instruction =
            "You are Ozzie, executing a subtask. Be concise and focused.".to_string();
        if let Some(wd) = work_dir {
            full_instruction.push_str(&format!(
                "\n\nWorking directory: {wd}\nAll file operations should use this directory."
            ));
        }
        full_instruction.push_str(&format!("\n\n## Task\n{instruction}"));

        let budget = self
            .provider_registry
            .budget_for(provider_name, TurnBudget::subtask());

        let caller_ctx = TOOL_CTX
            .try_with(|ctx| ctx.clone())
            .unwrap_or_default();

        let config = ReactConfig {
            provider: provider.clone(),
            tools: resolved_tools,
            instruction: full_instruction,
            budget,
            session_id: Some(caller_ctx.session_id.clone()),
            work_dir: work_dir.map(String::from).or(caller_ctx.work_dir),
            ..Default::default()
        };

        let messages = vec![Message::user(instruction)];

        let result = ReactLoop::run_from_messages(&config, messages).await;
        match result {
            ReactResult::Completed(r) | ReactResult::BudgetExhausted(r) => Ok(r.content),
            ReactResult::Cancelled { .. } => Ok("[cancelled]".to_string()),
            ReactResult::Yielded { .. } => Ok("[yielded]".to_string()),
            ReactResult::Error(e) => Err(ToolError::Execution(format!("subtask failed: {e}"))),
        }
    }
}

/// Inline sub-agent runner — executes a one-shot ReAct loop with dedicated persona and tools.
///
/// Dangerous tool approvals bubble up through the shared event bus, so the parent's
/// WebSocket clients see the prompt and the sub-agent blocks until the user responds.
struct DirectSubAgentRunner {
    provider_registry: Arc<ProviderRegistry>,
    tool_registry: Arc<ToolRegistry>,
    permissions: Arc<ToolPermissions>,
    approver: Arc<dyn ozzie_core::conscience::ApprovalRequester>,
    bus: Arc<Bus>,
    dangerous_tool_names: Vec<String>,
    sessions: Arc<dyn ozzie_runtime::SessionStore>,
}

#[async_trait::async_trait]
impl SubAgentRunner for DirectSubAgentRunner {
    async fn run_sub_agent(
        &self,
        agent_name: &str,
        config: &ozzie_core::config::SubAgentConfig,
        task: &str,
        context: Option<&str>,
        session_id: &str,
        work_dir: Option<&str>,
    ) -> Result<String, ToolError> {
        use ozzie_core::config::ContextMode;
        use ozzie_core::conscience::DangerousToolWrapper;

        // Resolve provider
        let provider_name = config
            .model
            .as_deref()
            .unwrap_or_else(|| self.provider_registry.default_name());
        let llm_provider = self
            .provider_registry
            .get(provider_name)
            .ok_or_else(|| ToolError::Execution(format!("unknown provider for agent '{agent_name}': {provider_name}")))?
            .clone();

        // Filter tools: only allowed, and never other agent_* tools (no nesting)
        let all_tools = self.tool_registry.all_tools();
        let resolved_tools: Vec<Arc<dyn ozzie_core::domain::Tool>> = if config.tools.is_empty() {
            all_tools
                .into_iter()
                .filter(|t| !t.info().name.starts_with("agent_"))
                .collect()
        } else {
            let allowed: std::collections::HashSet<&str> =
                config.tools.iter().map(|s| s.as_str()).collect();
            all_tools
                .into_iter()
                .filter(|t| {
                    let name = t.info().name;
                    allowed.contains(name.as_str()) && !name.starts_with("agent_")
                })
                .collect()
        };

        // Wrap dangerous tools with the shared approval system
        let wrapped_tools: Vec<Arc<dyn ozzie_core::domain::Tool>> = resolved_tools
            .into_iter()
            .map(|tool| {
                let name = tool.info().name.clone();
                let dangerous = self.dangerous_tool_names.contains(&name);
                DangerousToolWrapper::wrap_if_dangerous(
                    tool,
                    &name,
                    dangerous,
                    self.permissions.clone(),
                    self.bus.clone(),
                    self.approver.clone(),
                )
            })
            .collect();

        // Compose instruction — persona + task, no user profile, no memories
        let mut instruction = config.persona.clone();
        if let Some(wd) = work_dir {
            instruction.push_str(&format!(
                "\n\nWorking directory: {wd}\nAll file operations should use this directory."
            ));
        }
        instruction.push_str(&format!("\n\n## Task\n{task}"));
        if let Some(ctx) = context {
            instruction.push_str(&format!("\n\n## Context\n{ctx}"));
        }

        // For conversation mode, append conversation history (without system messages)
        let mut messages = vec![Message::user(task)];
        if config.context_mode == ContextMode::Conversation
            && let Ok(all_msgs) = self.sessions.load_messages(session_id).await
        {
            let history: Vec<Message> = all_msgs
                .into_iter()
                .filter(|m| m.role != ozzie_core::domain::ROLE_SYSTEM)
                .collect();
            if !history.is_empty() {
                instruction.push_str("\n\n## Conversation History\nThe following is the conversation history from the parent session for context.");
                let task_msg = messages.pop().expect("messages vec is non-empty");
                messages.extend(history);
                messages.push(task_msg);
            }
        }

        // Build budget
        let base_budget = TurnBudget::subtask();
        let budget = if let Some(ref bc) = config.budget {
            TurnBudget {
                max_turns: bc.max_turns.unwrap_or(base_budget.max_turns),
                max_output_tokens: bc.max_output_tokens.unwrap_or(base_budget.max_output_tokens),
                timeout: bc
                    .timeout_sec
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(base_budget.timeout),
            }
        } else {
            self.provider_registry.budget_for(provider_name, base_budget)
        };

        let react_config = ReactConfig {
            provider: llm_provider,
            tools: wrapped_tools,
            instruction,
            budget,
            session_id: Some(session_id.to_string()),
            work_dir: work_dir.map(String::from),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&react_config, messages).await;
        match result {
            ReactResult::Completed(r) | ReactResult::BudgetExhausted(r) => Ok(r.content),
            ReactResult::Cancelled { .. } => Ok("[cancelled]".to_string()),
            ReactResult::Yielded { .. } => Ok("[yielded]".to_string()),
            ReactResult::Error(e) => Err(ToolError::Execution(format!("sub-agent '{agent_name}' failed: {e}"))),
        }
    }
}

/// Schedule handler that runs a ReAct loop directly on trigger.
/// Uses the ActorPool for capacity management and ProviderRegistry for provider lookup.
struct DirectScheduleHandler {
    provider_registry: Arc<ProviderRegistry>,
    pool: Arc<ActorPool>,
    tool_registry: Arc<ToolRegistry>,
}

#[async_trait::async_trait]
impl ozzie_runtime::scheduler::ScheduleHandler for DirectScheduleHandler {
    async fn on_trigger(
        &self,
        entry: &ozzie_runtime::scheduler::ScheduleEntry,
        trigger: &str,
    ) {
        let template = match entry.source.task_template() {
            Some(t) => t,
            None => {
                warn!(entry_id = %entry.id, trigger, "schedule triggered but no task_template (skill-based?)");
                return;
            }
        };

        info!(
            entry_id = %entry.id,
            trigger,
            title = %template.title,
            "schedule triggered, running directly"
        );

        let provider_name = template
            .env
            .get("provider")
            .map(|s| s.as_str())
            .unwrap_or(self.provider_registry.default_name());

        // Acquire a capacity slot
        let slot = match self.pool.acquire(provider_name).await {
            Ok(s) => s,
            Err(e) => {
                error!(
                    entry_id = %entry.id,
                    provider = provider_name,
                    error = %e,
                    "failed to acquire actor slot for scheduled task"
                );
                return;
            }
        };

        let provider = match self.provider_registry.get(provider_name) {
            Some(p) => p.clone(),
            None => {
                error!(
                    entry_id = %entry.id,
                    provider = provider_name,
                    "unknown provider for scheduled task"
                );
                self.pool.release(slot);
                return;
            }
        };

        let all_tools = self.tool_registry.all_tools();
        let tools: Vec<_> = if template.tools.is_empty() {
            all_tools
        } else {
            let allowed: std::collections::HashSet<&str> =
                template.tools.iter().map(|s| s.as_str()).collect();
            all_tools
                .into_iter()
                .filter(|t| allowed.contains(t.info().name.as_str()))
                .collect()
        };

        let mut instruction =
            "You are Ozzie, executing a scheduled task. Be concise and focused.".to_string();
        if let Some(ref wd) = template.work_dir {
            instruction.push_str(&format!(
                "\n\nWorking directory: {wd}\nAll file operations should use this directory."
            ));
        }
        instruction.push_str(&format!(
            "\n\n## Task: {}\n{}",
            template.title, template.description
        ));

        let budget = self
            .provider_registry
            .budget_for(provider_name, TurnBudget::scheduled());

        let config = ReactConfig {
            provider,
            tools,
            instruction,
            budget,
            session_id: entry.session_id.clone(),
            work_dir: template.work_dir.clone(),
            ..Default::default()
        };

        let tool_names: Vec<_> = config.tools.iter().map(|t| t.info().name).collect();
        info!(
            entry_id = %entry.id,
            tool_count = tool_names.len(),
            tools = ?tool_names,
            work_dir = ?config.work_dir,
            instruction_len = config.instruction.len(),
            "scheduled task: starting ReactLoop"
        );

        let messages = vec![Message::user(&template.description)];

        match ReactLoop::run_from_messages(&config, messages).await {
            ReactResult::Completed(result) => {
                info!(
                    entry_id = %entry.id,
                    tool_calls = result.tool_calls_count,
                    content_len = result.content.len(),
                    "scheduled task completed"
                );
            }
            ReactResult::BudgetExhausted(result) => {
                warn!(
                    entry_id = %entry.id,
                    tool_calls = result.tool_calls_count,
                    "scheduled task budget exhausted"
                );
            }
            ReactResult::Cancelled { .. } => {
                info!(entry_id = %entry.id, "scheduled task cancelled");
            }
            ReactResult::Yielded { reason, .. } => {
                info!(entry_id = %entry.id, reason = %reason, "scheduled task yielded");
            }
            ReactResult::Error(e) => {
                if format!("{e}").contains("model unavailable")
                    || format!("{e}").contains("ModelUnavailable")
                {
                    self.pool.set_cooldown(provider_name);
                }
                error!(
                    entry_id = %entry.id,
                    error = %e,
                    "scheduled task failed"
                );
            }
        }

        self.pool.release(slot);
    }
}

// ---- Projects ----

fn resolve_workspaces_root(cfg: &config::Config) -> std::path::PathBuf {
    if let Some(ref root) = cfg.projects.workspaces_root {
        let expanded = expand_tilde(root);
        if expanded.is_absolute() {
            return expanded;
        }
    }
    ozzie_path().join("working")
}

fn init_projects(cfg: &config::Config) -> Arc<ozzie_core::project::ProjectRegistry> {
    let root = resolve_workspaces_root(cfg);
    let projects = ozzie_core::project::discover_projects(&root, &cfg.projects.extra_paths);
    let registry = Arc::new(ozzie_core::project::ProjectRegistry::new());
    for project in projects {
        registry.register(project);
    }
    registry
}

/// Expands `~` prefix to the user's home directory.
fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return std::path::PathBuf::from(home).join(rest);
    }
    std::path::PathBuf::from(path)
}
