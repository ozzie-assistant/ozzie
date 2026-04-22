use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use tracing::{debug, error, info, warn};

use ozzie_core::actors::{ActorInfo, ActorPool};
use ozzie_core::conscience::{ApprovalRequester, DangerousToolWrapper, ToolPermissions};
use ozzie_core::domain::{ContextCompressor, MemoryRetriever, Message, Tool};
use ozzie_core::events::{Event, EventBus, EventKind, EventPayload, EventSource};
use ozzie_core::layered::estimate_tokens;
use ozzie_core::profile::UserProfile;
use ozzie_core::prompt::{self, Composer, MemoryInfo};
use ozzie_llm::{ChatDelta, ChatMessage, Provider, ToolDefinition};

use dashmap::DashMap;

use crate::pairing_manager::PairingManager;
use crate::react::{self, PendingDrain as _, ReactObserver};
use crate::conversation::{Conversation, ConversationStore};
use crate::conversation_runtime::ConversationRuntime;

/// Configuration for building an EventRunner with dynamic prompt composition.
pub struct EventRunnerConfig {
    pub bus: Arc<dyn EventBus>,
    pub sessions: Arc<dyn ConversationStore>,
    pub provider: Arc<dyn Provider>,
    /// Full persona text (from SOUL.md or DEFAULT_PERSONA).
    pub persona: String,
    /// Agent instructions (from catalog).
    pub agent_instructions: String,
    /// Preferred language for responses (e.g. "fr").
    pub preferred_language: Option<String>,
    /// Skill name→description map for prompt injection.
    pub skill_descriptions: HashMap<String, String>,
    /// Optional custom instructions from config (overrides nothing, appended).
    pub custom_instructions: Option<String>,
    /// Available tools for the ReAct loop.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Optional memory retriever for implicit context injection.
    pub retriever: Option<Arc<dyn MemoryRetriever>>,
    /// Optional context compressor for long session histories.
    pub compressor: Option<Arc<dyn ContextCompressor>>,
    /// Tool permissions for dangerous-tool approval flow.
    /// When set, dangerous tools are wrapped with approval logic.
    pub permissions: Option<Arc<ToolPermissions>>,
    /// Approval requester for dangerous tools (event bus prompt).
    /// Required when `permissions` is set.
    pub approver: Option<Arc<dyn ApprovalRequester>>,
    /// Names of tools that require approval (dangerous tools).
    /// Used to determine which tools to wrap.
    pub dangerous_tool_names: Vec<String>,
    /// Optional pairing manager for connector message routing and policy resolution.
    pub pairing_manager: Option<Arc<PairingManager>>,
    /// Available actor info for prompt injection (multi-provider setups).
    pub actor_infos: Vec<ActorInfo>,
    /// Actor pool for capacity management. When set, the main session acquires
    /// a slot before running the ReactLoop, preventing subtasks/schedules from
    /// starving when `max_concurrent` is low.
    pub pool: Option<Arc<ActorPool>>,
    /// Provider name used for pool slot acquisition.
    pub provider_name: Option<String>,
    /// Provider's context window in tokens. Used to truncate history before LLM calls.
    pub context_window: Option<usize>,
    /// Optional user profile for system prompt injection.
    pub user_profile: Option<UserProfile>,
    /// Blob store for resolving image references before LLM calls.
    pub blob_store: Option<Arc<dyn ozzie_core::domain::BlobStore>>,
    /// Project registry for prompt injection when a session has an active project.
    pub project_registry: Option<Arc<ozzie_core::project::ProjectRegistry>>,
}

/// Listens for UserMessage events and runs the LLM, emitting
/// AssistantStream and AssistantMessage events back to the bus.
pub struct EventRunner {
    bus: Arc<dyn EventBus>,
    sessions: Arc<dyn ConversationStore>,
    provider: Arc<dyn Provider>,
    /// Pre-composed static portion of the system prompt (persona + instructions + language + skills + actors).
    static_prompt: String,
    /// Available tools for the ReAct loop.
    tools: Vec<Arc<dyn Tool>>,
    /// Pre-built tool definitions for the LLM.
    tool_defs: Vec<ToolDefinition>,
    /// Optional memory retriever for implicit context injection.
    retriever: Option<Arc<dyn MemoryRetriever>>,
    /// Optional context compressor for long session histories.
    compressor: Option<Arc<dyn ContextCompressor>>,
    /// Optional pairing manager for connector message routing and policy resolution.
    pairing_manager: Option<Arc<PairingManager>>,
    /// Per-session runtime state (cancellation, pending messages, active flag).
    session_runtimes: Arc<DashMap<String, Arc<ConversationRuntime>>>,
    /// Actor pool for capacity management.
    pool: Option<Arc<ActorPool>>,
    /// Provider name for pool slot acquisition.
    provider_name: Option<String>,
    /// Provider's context window in tokens.
    context_window: Option<usize>,
    /// Blob store for resolving image references.
    blob_store: Option<Arc<dyn ozzie_core::domain::BlobStore>>,
    /// Project registry for prompt injection.
    project_registry: Option<Arc<ozzie_core::project::ProjectRegistry>>,
}

impl EventRunner {
    /// Creates an EventRunner with dynamic prompt composition.
    pub fn with_config(config: EventRunnerConfig) -> Self {
        // Wrap dangerous tools with approval logic if permissions are configured
        let tools = if let (Some(perms), Some(approver)) =
            (&config.permissions, &config.approver)
        {
            config
                .tools
                .into_iter()
                .map(|tool| {
                    let name = tool.info().name.clone();
                    let dangerous = config.dangerous_tool_names.contains(&name);
                    DangerousToolWrapper::wrap_if_dangerous(
                        tool,
                        &name,
                        dangerous,
                        perms.clone(),
                        config.bus.clone(),
                        approver.clone(),
                    )
                })
                .collect()
        } else {
            config.tools
        };

        let tool_defs = build_tool_definitions(&tools);
        let tool_descriptions: HashMap<String, String> = tools
            .iter()
            .map(|t| {
                let info = t.info();
                (info.name, info.description)
            })
            .collect();
        let tool_names: Vec<String> = tools.iter().map(|t| t.info().name).collect();

        // Build static prompt sections
        let mut composer = Composer::new()
            .add_section("Persona", &config.persona)
            .add_section("Agent Instructions", &config.agent_instructions);

        // User profile section
        if let Some(ref profile) = config.user_profile {
            let section = prompt::user_profile_section(profile);
            composer = composer.add_section("User Profile", &section);
        }

        // Language section
        if let Some(ref lang) = config.preferred_language {
            composer = composer.add_section(
                "Language",
                &format!("Always respond in {lang}. Use {lang} for all explanations and conversations. Code, identifiers, and technical terms remain in English."),
            );
        }

        // Skills section
        if !config.skill_descriptions.is_empty() {
            let section = prompt::skill_section(&config.skill_descriptions, false);
            composer = composer.add_section("Skills", &section);
        }

        // Tools section
        if !tool_names.is_empty() {
            let section = prompt::tool_section(&tool_names, &tool_descriptions, false);
            composer = composer.add_section("Tools", &section);
        }

        // Actors section (multi-provider routing info)
        if !config.actor_infos.is_empty() {
            let section = prompt::actor_section(&config.actor_infos);
            composer = composer.add_section("Actors", &section);
        }

        // Custom instructions (appended last)
        if let Some(ref custom) = config.custom_instructions {
            composer = composer.add_section("Custom Instructions", custom);
        }

        composer.log_manifest("event_runner static prompt");
        let static_prompt = composer.build();

        Self {
            bus: config.bus,
            sessions: config.sessions,
            provider: config.provider,
            static_prompt,
            tools,
            tool_defs,
            retriever: config.retriever,
            compressor: config.compressor,
            pairing_manager: config.pairing_manager,
            session_runtimes: Arc::new(DashMap::new()),
            pool: config.pool,
            provider_name: config.provider_name,
            context_window: config.context_window,
            blob_store: config.blob_store,
            project_registry: config.project_registry,
        }
    }

    /// Legacy constructor for backward compatibility (static prompt, no tools).
    pub fn new(
        bus: Arc<dyn EventBus>,
        sessions: Arc<dyn ConversationStore>,
        provider: Arc<dyn Provider>,
        system_prompt: String,
    ) -> Self {
        Self {
            bus,
            sessions,
            provider,
            static_prompt: system_prompt,
            tools: Vec::new(),
            tool_defs: Vec::new(),
            retriever: None,
            compressor: None,
            pairing_manager: None,
            session_runtimes: Arc::new(DashMap::new()),
            pool: None,
            provider_name: None,
            context_window: None,
            blob_store: None,
            project_registry: None,
        }
    }

    /// Starts the event runner loop. Spawns a tokio task that listens for
    /// UserMessage events and processes them.
    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            self.run_loop().await;
        });
    }

    /// Returns the per-session runtime, creating one if needed.
    pub fn get_or_create_runtime(&self, session_id: &str) -> Arc<ConversationRuntime> {
        self.session_runtimes
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(ConversationRuntime::new()))
            .clone()
    }

    /// Cancels the active ReactLoop for the given session (if any).
    pub fn cancel_session(&self, session_id: &str) {
        if let Some(rt) = self.session_runtimes.get(session_id) {
            rt.cancel();
        }
    }

    /// Returns a reference to the session runtimes map (for gateway integration).
    pub fn session_runtimes(&self) -> &Arc<DashMap<String, Arc<ConversationRuntime>>> {
        &self.session_runtimes
    }

    async fn run_loop(&self) {
        let mut rx = self.bus.subscribe(&[EventKind::UserMessage.as_str(), EventKind::ConnectorMessage.as_str(), EventKind::SessionClear.as_str()]);
        info!("event runner started");

        loop {
            match rx.recv().await {
                Ok(event) => {
                    if matches!(&event.payload, EventPayload::UserMessage { .. }) {
                        self.dispatch_user_message(event);
                    } else if matches!(&event.payload, EventPayload::ConnectorMessage { .. }) {
                        let runner = self.clone_ref();
                        tokio::spawn(async move {
                            runner.handle_connector_message(event).await;
                        });
                    } else if matches!(&event.payload, EventPayload::SessionClear { .. }) {
                        let runner = self.clone_ref();
                        tokio::spawn(async move {
                            runner.handle_session_clear(event).await;
                        });
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "event runner lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("event bus closed, stopping event runner");
                    break;
                }
            }
        }
    }

    /// Dispatches a user message: buffers if session is busy, spawns handler otherwise.
    fn dispatch_user_message(&self, event: Event) {
        let session_id = match &event.session_id {
            Some(sid) => sid.clone(),
            None => return,
        };
        let text = match &event.payload {
            EventPayload::UserMessage { text, .. } => text.clone(),
            _ => return,
        };
        if text.is_empty() {
            return;
        }

        let runtime = self.get_or_create_runtime(&session_id);

        if runtime.is_active() {
            // Conversation busy — buffer message for next turn
            debug!(session_id = %session_id, "session active, buffering user message");
            runtime.push_pending(text);
        } else {
            // Conversation idle — start processing (reset clears stale cancel tokens)
            runtime.reset();
            runtime.set_active(true);
            let runner = self.clone_ref();
            let rt = runtime.clone();
            let sid = session_id.clone();
            tokio::spawn(async move {
                runner.handle_user_message(event, rt.clone()).await;
                rt.set_active(false);

                // Check for messages that arrived during processing.
                // If pending messages exist, re-publish them so run_loop picks them up.
                let pending = rt.drain();
                if !pending.is_empty() {
                    debug!(session_id = %sid, count = pending.len(), "re-publishing buffered messages after loop end");
                    for text in pending {
                        runner.bus.publish(Event::with_session(
                            EventSource::Hub,
                            EventPayload::user_message(text),
                            &sid,
                        ));
                    }
                }
            });
        }
    }

    fn clone_ref(&self) -> Arc<Self> {
        Arc::new(Self {
            bus: self.bus.clone(),
            sessions: self.sessions.clone(),
            provider: self.provider.clone(),
            static_prompt: self.static_prompt.clone(),
            tools: self.tools.clone(),
            tool_defs: self.tool_defs.clone(),
            retriever: self.retriever.clone(),
            compressor: self.compressor.clone(),
            pairing_manager: self.pairing_manager.clone(),
            session_runtimes: self.session_runtimes.clone(),
            pool: self.pool.clone(),
            provider_name: self.provider_name.clone(),
            context_window: self.context_window,
            blob_store: self.blob_store.clone(),
            project_registry: self.project_registry.clone(),
        })
    }

    /// Composes the full system prompt for a specific session turn.
    /// Includes dynamic session context and relevant memories (if any).
    fn compose_system_prompt(
        &self,
        session: &Conversation,
        message_count: usize,
        memories: &[MemoryInfo],
    ) -> String {
        let session_section = prompt::session_section(
            session.root_dir.as_deref(),
            session.language.as_deref(),
            session.title.as_deref(),
            message_count,
        );
        let memory_sec = prompt::memory_section(memories, 0);

        let project_sec = session
            .project_id
            .as_ref()
            .and_then(|pid| {
                self.project_registry
                    .as_ref()
                    .and_then(|reg| reg.get(pid))
            })
            .map(|p| prompt::project_section(&p.name, &p.description, &p.path, &p.instructions))
            .unwrap_or_default();

        let dynamic_parts: Vec<&str> = [
            session_section.as_str(),
            project_sec.as_str(),
            memory_sec.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();

        if dynamic_parts.is_empty() {
            return self.static_prompt.clone();
        }

        format!("{}\n\n{}", self.static_prompt, dynamic_parts.join("\n\n"))
    }

    /// Truncates conversation history to fit within the provider's context window.
    ///
    /// Estimates token usage for system prompt + tool definitions + history,
    /// then drops oldest messages until the total fits within 80% of the window
    /// (reserving 20% for output tokens).
    fn truncate_to_context_window(
        &self,
        mut history: Vec<Message>,
        system_prompt: &str,
    ) -> Vec<Message> {
        let ctx_window = match self.context_window {
            Some(cw) if cw > 0 => cw,
            _ => return history,
        };

        let system_tokens = estimate_tokens(system_prompt);
        let tool_tokens = estimate_tool_tokens(&self.tool_defs);
        let overhead = system_tokens + tool_tokens;

        // Reserve 20% of the window for output tokens
        let input_budget = ((ctx_window as f64 * 0.80) as usize).saturating_sub(overhead);

        let total_history_tokens: usize = history.iter().map(|m| estimate_tokens(&m.content)).sum();

        if total_history_tokens <= input_budget {
            return history;
        }

        // Drop oldest messages until we fit
        let original_len = history.len();
        while history.len() > 1 {
            let current: usize = history.iter().map(|m| estimate_tokens(&m.content)).sum();
            if current <= input_budget {
                break;
            }
            history.remove(0);
        }

        let dropped = original_len - history.len();
        if dropped > 0 {
            warn!(
                context_window = ctx_window,
                overhead_tokens = overhead,
                input_budget,
                dropped_messages = dropped,
                remaining = history.len(),
                "context window: truncated history to fit budget"
            );
        }

        history
    }

    /// Retrieves relevant memories for the current user message.
    /// Enriches the query with session context (title, root dir basename, language tag).
    /// Filters results by relevance threshold (>= 0.3).
    async fn retrieve_memories(
        &self,
        session: &Conversation,
        user_content: &str,
        history: &[Message],
    ) -> Vec<MemoryInfo> {
        let retriever = match &self.retriever {
            Some(r) => r,
            None => return Vec::new(),
        };

        if user_content.is_empty() {
            return Vec::new();
        }

        // Enrich query with session context for better semantic matching
        let query = enrich_query_with_session(user_content, session);

        // Add recent user context (up to 2 previous messages, 200 chars each)
        let recent = recent_user_context(history, 2);
        let full_query = if recent.is_empty() {
            query
        } else {
            format!("{query} {recent}")
        };

        // Extract tags from session
        let tags = extract_session_tags(session);

        const MEMORY_LIMIT: usize = 5;
        const RELEVANCE_THRESHOLD: f64 = 0.3;

        let memories = match retriever.retrieve(&full_query, &tags, MEMORY_LIMIT).await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "memory retrieval failed");
                return Vec::new();
            }
        };

        memories
            .into_iter()
            .filter(|m| m.score >= RELEVANCE_THRESHOLD)
            .map(|m| MemoryInfo {
                memory_type: m.entry.memory_type.as_str().to_string(),
                title: m.entry.title,
                content: m.content,
            })
            .collect()
    }

    /// Routes an incoming connector message to the ReAct pipeline.
    ///
    /// Checks the pairing (must be approved), derives a stable session ID from
    /// the identity, creates the session on first contact, and republishes as a
    /// `UserMessage` event so `handle_user_message` picks it up.
    async fn handle_connector_message(&self, event: Event) {
        let (connector, channel_id, message_id, content, identity_json, roles) = match &event.payload {
            EventPayload::ConnectorMessage {
                connector,
                channel_id,
                message_id,
                content,
                identity,
                roles,
                ..
            } => (
                connector.clone(),
                channel_id.clone(),
                message_id.clone(),
                content.clone(),
                identity.clone(),
                roles.clone(),
            ),
            _ => return,
        };

        if content.is_empty() {
            return;
        }

        // Deserialize identity
        let identity: ozzie_core::connector::Identity =
            match identity_json.as_ref().and_then(|v| serde_json::from_value(v.clone()).ok()) {
                Some(id) => id,
                None => {
                    warn!(connector = %connector, "connector message without identity, ignoring");
                    return;
                }
            };

        // Resolve policy — only paired identities are served
        let pm = match &self.pairing_manager {
            Some(pm) => pm,
            None => {
                warn!("no pairing manager configured, ignoring connector message");
                return;
            }
        };
        let policy_name = match pm.resolve_policy(&identity, &roles) {
            Some(p) => p,
            None => {
                let is_pair = content.trim().eq_ignore_ascii_case("/pair");
                if is_pair {
                    let incoming = ozzie_core::connector::IncomingMessage {
                        identity: identity.clone(),
                        content: content.clone(),
                        channel_id: channel_id.clone(),
                        message_id: message_id.clone(),
                        timestamp: chrono::Utc::now(),
                        roles: roles.clone(),
                        ..Default::default()
                    };
                    let request_id = pm.on_pair_request(&incoming);
                    info!(
                        request_id = %request_id,
                        platform = %identity.platform,
                        user = %identity.user_id,
                        "pairing request created from connector"
                    );
                    self.bus.publish(Event::new(
                        EventSource::Agent,
                        EventPayload::ConnectorReply {
                            connector: connector.clone(),
                            channel_id: channel_id.clone(),
                            content: format!(
                                "Pairing request created (`{request_id}`). Waiting for owner approval."
                            ),
                            reply_to_id: if message_id.is_empty() { None } else { Some(message_id.clone()) },
                            feedback: false,
                        },
                    ));
                } else {
                    info!(
                        platform = %identity.platform,
                        server_id = %identity.server_id,
                        user = %identity.user_id,
                        "unpaired identity, sending hint"
                    );
                    self.bus.publish(Event::new(
                        EventSource::Agent,
                        EventPayload::ConnectorReply {
                            connector: connector.clone(),
                            channel_id: channel_id.clone(),
                            content: "I don't know you yet. Send `/pair` to request access.".to_string(),
                            reply_to_id: if message_id.is_empty() { None } else { Some(message_id.clone()) },
                            feedback: false,
                        },
                    ));
                }
                return;
            }
        };

        // Stable session ID derived from identity coordinates
        let session_id = format!(
            "{}_{}_{}_{}", identity.platform, identity.server_id, identity.channel_id, identity.user_id
        );

        // Create session on first contact; store connector routing info + policy.
        match self.sessions.get(&session_id).await {
            Ok(Some(mut s)) => {
                let mut changed = false;
                if s.policy_name.is_none() {
                    s.policy_name = Some(policy_name);
                    changed = true;
                }
                if !s.metadata.contains_key("connector") {
                    s.metadata.insert("connector".to_string(), connector.clone());
                    s.metadata.insert(
                        "reply_channel_id".to_string(),
                        identity.channel_id.clone(),
                    );
                    changed = true;
                }
                // Always update incoming_message_id for each new message.
                s.metadata.insert("incoming_message_id".to_string(), message_id.clone());
                if let Err(e) = self.sessions.update(&s).await {
                    warn!(session_id = %session_id, error = %e, "failed to update session metadata");
                }
                let _ = changed; // consumed above
            }
            Ok(None) => {
                let mut s = Conversation::new(session_id.clone());
                s.policy_name = Some(policy_name);
                s.metadata
                    .insert("connector".to_string(), connector.clone());
                s.metadata.insert(
                    "reply_channel_id".to_string(),
                    identity.channel_id.clone(),
                );
                s.metadata.insert("incoming_message_id".to_string(), message_id.clone());
                if let Err(e) = self.sessions.create(&s).await {
                    error!(error = %e, "failed to create connector session");
                    return;
                }
            }
            Err(e) => {
                error!(error = %e, "failed to load connector session");
                return;
            }
        }

        // Signal typing + thinking reaction on the user's original message.
        if !message_id.is_empty() {
            self.bus.publish(Event::new(
                EventSource::Agent,
                EventPayload::ConnectorTyping {
                    connector: connector.clone(),
                    channel_id: channel_id.clone(),
                },
            ));
            self.bus.publish(Event::new(
                EventSource::Agent,
                EventPayload::ConnectorAddReaction {
                    connector: connector.clone(),
                    channel_id: channel_id.clone(),
                    message_id: message_id.clone(),
                    reaction: ozzie_core::connector::Reaction::Thinking,
                },
            ));
        }

        // Dispatch as a normal UserMessage for the standard processing path
        self.bus.publish(Event::with_session(
            EventSource::Connector,
            EventPayload::user_message(content),
            &session_id,
        ));
    }

    /// Handles `session.clear`: advances `history_start_index` so future LLM calls
    /// skip old messages. The messages file is kept intact on disk for audit/logging.
    async fn handle_session_clear(&self, event: Event) {
        let (session_id, connector, channel_id) = match &event.payload {
            EventPayload::SessionClear {
                session_id,
                connector,
                channel_id,
            } => (session_id.clone(), connector.clone(), channel_id.clone()),
            _ => return,
        };

        // Load current message count to set as the new history start.
        let start_index = match self.sessions.load_messages(&session_id).await {
            Ok(msgs) => msgs.len(),
            Err(_) => 0,
        };

        // Update session metadata to record where the new conversation starts.
        match self.sessions.get(&session_id).await {
            Ok(Some(mut session)) => {
                session
                    .metadata
                    .insert("history_start_index".to_string(), start_index.to_string());
                session.updated_at = chrono::Utc::now();
                if let Err(e) = self.sessions.update(&session).await {
                    warn!(session_id = %session_id, error = %e, "failed to update session metadata");
                }
            }
            Ok(None) => {
                // Conversation not yet created — nothing to clear, reply anyway.
            }
            Err(e) => {
                warn!(session_id = %session_id, error = %e, "failed to load session for clear");
            }
        }

        info!(session_id = %session_id, history_start_index = %start_index, "conversation cleared");

        self.bus.publish(Event::new(
            EventSource::Agent,
            EventPayload::ConnectorReply {
                connector,
                channel_id,
                content: "🧹 Conversation cleared. Starting fresh!".to_string(),
                reply_to_id: None,
                feedback: false,
            },
        ));
    }

    async fn handle_user_message(&self, event: Event, runtime: Arc<ConversationRuntime>) {
        let session_id = match &event.session_id {
            Some(sid) => sid.clone(),
            None => {
                warn!("user message without session_id");
                return;
            }
        };

        let (content, images) = match &event.payload {
            EventPayload::UserMessage { text, images } => (text.clone(), images.clone()),
            _ => return,
        };

        if content.is_empty() && images.is_empty() {
            return;
        }

        debug!(session_id = %session_id, images = images.len(), "processing user message");

        // Persist user message
        let user_msg = Message::user(&content);
        if let Err(e) = self.sessions.append_message(&session_id, user_msg).await {
            error!(error = %e, "failed to persist user message");
        }

        // Load session for dynamic prompt composition
        let session = match self.sessions.get(&session_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                warn!(session_id = %session_id, "session not found");
                Conversation::new(session_id.clone())
            }
            Err(e) => {
                error!(error = %e, "failed to load session");
                Conversation::new(session_id.clone())
            }
        };

        // Load conversation history, applying /clear offset if set.
        let history = match self.sessions.load_messages(&session_id).await {
            Ok(msgs) => {
                let start = session
                    .metadata
                    .get("history_start_index")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(msgs.len());
                if start > 0 {
                    msgs.into_iter().skip(start).collect()
                } else {
                    msgs
                }
            }
            Err(e) => {
                error!(error = %e, "failed to load messages");
                self.emit_error(&session_id, &format!("failed to load history: {e}"));
                return;
            }
        };

        // Compress conversation history if a compressor is configured
        let history = if let Some(ref compressor) = self.compressor {
            match compressor.compress(&session_id, &history).await {
                Ok(compressed) => {
                    debug!(
                        session_id = %session_id,
                        original = history.len(),
                        compressed = compressed.len(),
                        "context compressed"
                    );
                    compressed
                }
                Err(e) => {
                    warn!(error = %e, "context compression failed, using full history");
                    history
                }
            }
        } else {
            history
        };

        // Retrieve relevant memories for implicit context injection
        let memories = self.retrieve_memories(&session, &content, &history).await;

        // Compose full system prompt (static + dynamic session context + memories)
        let system_prompt = self.compose_system_prompt(&session, history.len(), &memories);

        // Truncate history if approaching context window limit
        let history = self.truncate_to_context_window(history, &system_prompt);

        // Build chat messages
        let mut chat_messages = Vec::new();
        if !system_prompt.is_empty() {
            chat_messages.push(ChatMessage::text(ozzie_llm::ChatRole::System, system_prompt));
        }
        for msg in &history {
            let role = match msg.role.as_str() {
                "system" => ozzie_llm::ChatRole::System,
                "assistant" => ozzie_llm::ChatRole::Assistant,
                "tool" => ozzie_llm::ChatRole::Tool,
                _ => ozzie_llm::ChatRole::User,
            };
            chat_messages.push(ChatMessage::text(role, &msg.content));
        }

        // Resolve image blobs to inline base64 and append to last user message
        if !images.is_empty()
            && let Some(ref store) = self.blob_store
            && let Some(last_user) = chat_messages.iter_mut().rev().find(|m| m.role == ozzie_llm::ChatRole::User)
        {
            for blob in &images {
                match crate::blob_store::resolve_blob_to_content(blob, store.as_ref()).await {
                    Ok(content) => last_user.content.push(content),
                    Err(e) => warn!(error = %e, "failed to resolve image blob, skipping"),
                }
            }
        }

        // Resolve tools for this session's policy (if set).
        let (session_tools, session_tool_defs) = self.tools_for_session(&session);

        // If we have tools, use the ReAct loop; otherwise stream directly
        if !session_tools.is_empty() {
            // Resolve git_auto_commit from active project
            let git_auto_commit = session
                .project_id
                .as_ref()
                .and_then(|pid| {
                    self.project_registry
                        .as_ref()
                        .and_then(|reg| reg.get(pid))
                })
                .is_some_and(|p| p.git_auto_commit);

            self.process_with_tools(&session_id, chat_messages, &session_tools, &session_tool_defs, session.root_dir.clone(), git_auto_commit, &runtime)
                .await;
        } else {
            self.process_without_tools(&session_id, chat_messages).await;
        }
    }

    /// Returns the tools (and their definitions) allowed for a session.
    ///
    /// If the session has no policy, all tools are returned unchanged.
    /// If the policy is unrecognised, all tools are returned (fail-open for unknown policies).
    fn tools_for_session(&self, session: &Conversation) -> (Vec<Arc<dyn Tool>>, Vec<ToolDefinition>) {
        let Some(ref policy_name) = session.policy_name else {
            return (self.tools.clone(), self.tool_defs.clone());
        };
        let Some(policy) = ozzie_core::policy::Policy::by_name(policy_name) else {
            return (self.tools.clone(), self.tool_defs.clone());
        };
        let tools: Vec<Arc<dyn Tool>> = self
            .tools
            .iter()
            .filter(|t| policy.allows_tool(&t.info().name))
            .cloned()
            .collect();
        let tool_defs = build_tool_definitions(&tools);
        (tools, tool_defs)
    }

    /// Processes a message using the ReAct tool-calling loop.
    ///
    /// Delegates to `ReactLoop::run()` with an observer that bridges events
    /// to the event bus.
    #[allow(clippy::too_many_arguments)]
    async fn process_with_tools(
        &self,
        session_id: &str,
        chat_messages: Vec<ChatMessage>,
        tools: &[Arc<dyn Tool>],
        _tool_defs: &[ToolDefinition],
        work_dir: Option<String>,
        git_auto_commit: bool,
        runtime: &Arc<ConversationRuntime>,
    ) {
        // Acquire actor pool slot so subtasks/schedules see capacity constraints.
        let pool_slot = if let (Some(pool), Some(pname)) = (&self.pool, &self.provider_name) {
            match pool.acquire(pname).await {
                Ok(slot) => Some((pool.clone(), slot)),
                Err(e) => {
                    warn!(session_id, error = %e, "failed to acquire actor slot, proceeding without");
                    None
                }
            }
        } else {
            None
        };

        // Emit stream start
        self.emit_stream(session_id, "start", "", 0);

        let observer = Arc::new(EventRunnerObserver {
            bus: self.bus.clone(),
            sessions: self.sessions.clone(),
            session_id: session_id.to_string(),
        });

        let budget = react::TurnBudget::default();

        let config = react::ReactConfig {
            provider: self.provider.clone(),
            tools: tools.to_vec(),
            instruction: String::new(), // already in chat_messages
            budget,
            observer: Some(observer),
            session_id: Some(session_id.to_string()),
            work_dir,
            pending_source: Some(runtime.clone() as Arc<dyn react::PendingDrain>),
            cancel_token: Some(runtime.cancel_token().clone()),
            repetition_window: 10,
            repetition_threshold: 3,
            git_auto_commit,
        };

        let result = react::ReactLoop::run(&config, chat_messages).await;

        match result {
            react::ReactResult::Completed(r) => {
                self.emit_stream(session_id, "end", "", 0);
                self.finalize_response(session_id, &r.content).await;
            }
            react::ReactResult::BudgetExhausted(r) => {
                self.emit_stream(session_id, "end", "", 0);
                self.finalize_response(session_id, &r.content).await;
            }
            react::ReactResult::Cancelled { turns, reason } => {
                info!(session_id = %session_id, turns, reason = %reason, "react loop cancelled");
                self.emit_stream(session_id, "end", "", 0);
                self.finalize_response(session_id, "[cancelled by user]").await;
            }
            react::ReactResult::Yielded { turns, reason, .. } => {
                info!(session_id = %session_id, turns, reason = %reason, "react loop yielded");
                self.emit_stream(session_id, "end", "", 0);
                self.finalize_response(session_id, &format!("[yielded: {reason}]")).await;
            }
            react::ReactResult::Error(e) => {
                error!(error = %e, "react loop error");
                self.emit_error(session_id, &format!("react loop error: {e}"));
            }
        }

        // Release actor pool slot
        if let Some((pool, slot)) = pool_slot {
            pool.release(slot);
        }
    }

    /// Processes a message without tools (streaming or buffered).
    async fn process_without_tools(&self, session_id: &str, chat_messages: Vec<ChatMessage>) {
        // Try streaming first, fall back to buffered
        match self.provider.chat_stream(&chat_messages, &[]).await {
            Ok(stream) => {
                self.process_stream(session_id, stream).await;
            }
            Err(e) => {
                debug!(error = %e, "streaming not available, falling back to buffered");
                self.process_buffered(session_id, &chat_messages).await;
            }
        }
    }

    async fn process_stream(
        &self,
        session_id: &str,
        stream: std::pin::Pin<
            Box<dyn futures_core::Stream<Item = Result<ChatDelta, ozzie_llm::LlmError>> + Send>,
        >,
    ) {
        // Emit stream start
        self.emit_stream(session_id, "start", "", 0);

        let mut full_content = String::new();
        let mut index = 0u64;
        let mut stream = stream;

        while let Some(result) = stream.next().await {
            match result {
                Ok(ChatDelta::Content(text)) => {
                    full_content.push_str(&text);
                    index += 1;
                    self.emit_stream(session_id, "delta", &text, index);
                }
                Ok(ChatDelta::Done { usage, .. }) => {
                    self.emit_llm_call(
                        session_id,
                        "response",
                        usage.input_tokens,
                        usage.output_tokens,
                    );
                    break;
                }
                Ok(_) => {
                    // ToolCallStart, ToolCallDelta — skip for now
                }
                Err(e) => {
                    error!(error = %e, "stream error");
                    self.emit_error(session_id, &format!("stream error: {e}"));
                    return;
                }
            }
        }

        // Emit stream end
        self.emit_stream(session_id, "end", "", index + 1);

        // Persist and emit final message
        self.finalize_response(session_id, &full_content).await;
    }

    async fn process_buffered(&self, session_id: &str, messages: &[ChatMessage]) {
        match self.provider.chat(messages, &[]).await {
            Ok(response) => {
                // Emit LLM call event for cost tracking
                self.emit_llm_call(
                    session_id,
                    "response",
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                );

                // Emit as a single stream sequence
                self.emit_stream(session_id, "start", "", 0);
                if !response.content.is_empty() {
                    self.emit_stream(session_id, "delta", &response.content, 1);
                }
                self.emit_stream(session_id, "end", "", 2);

                self.finalize_response(session_id, &response.content).await;
            }
            Err(e) => {
                error!(error = %e, "LLM call failed");
                self.emit_error(session_id, &format!("LLM error: {e}"));
            }
        }
    }

    async fn finalize_response(&self, session_id: &str, content: &str) {
        // Persist assistant message
        if !content.is_empty() {
            let msg = Message::assistant(content);
            if let Err(e) = self.sessions.append_message(session_id, msg).await {
                error!(error = %e, "failed to persist assistant message");
            }
        }

        // Update session message_count from persisted messages
        if let Ok(Some(mut session)) = self.sessions.get(session_id).await
            && let Ok(msgs) = self.sessions.load_messages(session_id).await
        {
            session.message_count = msgs.len();
            session.updated_at = chrono::Utc::now();
            if let Err(e) = self.sessions.update(&session).await {
                warn!(error = %e, "failed to update session message_count");
            }
        }

        // Emit assistant.message event (for WS clients: TUI, ozzie ask, etc.)
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::AssistantMessage {
                content: content.to_string(),
                error: None,
            },
            session_id,
        ));

        // If this session was initiated by a connector, route the response back.
        if !content.is_empty() {
            match self.sessions.get(session_id).await {
                Ok(Some(session)) => {
                    match (
                        session.metadata.get("connector"),
                        session.metadata.get("reply_channel_id"),
                    ) {
                        (Some(connector), Some(channel_id)) => {
                            info!(
                                session_id = %session_id,
                                connector = %connector,
                                "routing response to connector"
                            );
                            // Clear status reactions from the user's original message before sending the reply.
                            if let Some(msg_id) = session.metadata.get("incoming_message_id")
                                && !msg_id.is_empty()
                            {
                                self.bus.publish(Event::new(
                                    EventSource::Agent,
                                    EventPayload::ConnectorClearReactions {
                                        connector: connector.clone(),
                                        channel_id: channel_id.clone(),
                                        message_id: msg_id.clone(),
                                    },
                                ));
                            }
                            self.bus.publish(Event::new(
                                EventSource::Agent,
                                EventPayload::ConnectorReply {
                                    connector: connector.clone(),
                                    channel_id: channel_id.clone(),
                                    content: content.to_string(),
                                    reply_to_id: None,
                                    feedback: false,
                                },
                            ));
                        }
                        _ => {
                            debug!(session_id = %session_id, "no connector routing info in session");
                        }
                    }
                }
                Ok(None) => {
                    debug!(session_id = %session_id, "session not found for connector routing");
                }
                Err(e) => {
                    warn!(session_id = %session_id, error = %e, "failed to load session for connector routing");
                }
            }
        }

        info!(session_id = %session_id, len = content.len(), "response complete");
    }

    fn emit_stream(&self, session_id: &str, phase: &str, content: &str, index: u64) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::AssistantStream {
                phase: phase.to_string(),
                content: content.to_string(),
                index,
            },
            session_id,
        ));
    }

    /// Maps a tool name to the appropriate semantic Reaction.
    fn tool_to_reaction(tool_name: &str) -> ozzie_core::connector::Reaction {
        use ozzie_core::connector::Reaction;
        if tool_name.contains("web") || tool_name.contains("search") || tool_name.contains("fetch") {
            Reaction::Web
        } else if tool_name.contains("shell") || tool_name.contains("command") || tool_name.contains("run") {
            Reaction::Command
        } else if tool_name.contains("file") || tool_name.contains("edit") || tool_name.contains("write") || tool_name.contains("str_replace") {
            Reaction::Edit
        } else if tool_name.contains("task") {
            Reaction::Task
        } else if tool_name.contains("memory") {
            Reaction::Memory
        } else if tool_name.contains("schedule") {
            Reaction::Schedule
        } else {
            Reaction::Tool
        }
    }

    /// Emits an internal.llm.call event for cost tracking.
    fn emit_llm_call(
        &self,
        session_id: &str,
        phase: &str,
        tokens_input: u64,
        tokens_output: u64,
    ) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::LlmCall {
                phase: phase.to_string(),
                tokens_input,
                tokens_output,
            },
            session_id,
        ));
    }

    fn emit_error(&self, session_id: &str, message: &str) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::AssistantMessage {
                content: String::new(),
                error: Some(message.to_string()),
            },
            session_id,
        ));
    }
}

/// Observer that bridges ReactLoop events to the event bus.
///
/// Created per-session in `process_with_tools` to emit tool call events,
/// LLM cost tracking events, streaming deltas, and connector reactions.
struct EventRunnerObserver {
    bus: Arc<dyn EventBus>,
    sessions: Arc<dyn ConversationStore>,
    session_id: String,
}

#[async_trait::async_trait]
impl ReactObserver for EventRunnerObserver {
    fn on_llm_response(&self, input_tokens: u64, output_tokens: u64) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::LlmCall {
                phase: "response".to_string(),
                tokens_input: input_tokens,
                tokens_output: output_tokens,
            },
            &self.session_id,
        ));
    }

    fn on_stream_delta(&self, content: &str, index: u64) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::AssistantStream {
                phase: "delta".to_string(),
                content: content.to_string(),
                index,
            },
            &self.session_id,
        ));
    }

    fn on_tool_call(&self, call_id: &str, tool: &str, arguments: &str) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::ToolCall {
                call_id: call_id.to_string(),
                tool: tool.to_string(),
                arguments: arguments.to_string(),
            },
            &self.session_id,
        ));
    }

    fn on_tool_result(&self, call_id: &str, tool: &str, result: &str, is_error: bool) {
        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::ToolResult {
                call_id: call_id.to_string(),
                tool: tool.to_string(),
                result: result.to_string(),
                is_error,
            },
            &self.session_id,
        ));
    }

    async fn on_pending_drained(&self, text: &str) {
        // Persist the buffered user message
        let msg = Message::user(text);
        if let Err(e) = self.sessions.append_message(&self.session_id, msg).await {
            error!(error = %e, "failed to persist buffered user message");
        }
    }

    async fn on_tool_pre_execute(&self, tool_name: &str) {
        // Update connector status reaction if this session is connector-initiated
        if let Ok(Some(s)) = self.sessions.get(&self.session_id).await
            && let (Some(conn), Some(ch), Some(msg_id)) = (
                s.metadata.get("connector"),
                s.metadata.get("reply_channel_id"),
                s.metadata.get("incoming_message_id"),
            )
            && !msg_id.is_empty()
        {
            self.bus.publish(Event::new(
                EventSource::Agent,
                EventPayload::ConnectorAddReaction {
                    connector: conn.clone(),
                    channel_id: ch.clone(),
                    message_id: msg_id.clone(),
                    reaction: EventRunner::tool_to_reaction(tool_name),
                },
            ));
        }
    }

    fn progress_sender(&self) -> Option<ozzie_core::domain::ProgressSender> {
        let bus = self.bus.clone();
        let session_id = self.session_id.clone();
        Some(Arc::new(move |p: ozzie_core::domain::ToolProgress| {
            bus.publish(Event::with_session(
                EventSource::Agent,
                EventPayload::ToolProgress {
                    call_id: p.call_id,
                    tool: p.tool,
                    message: p.message,
                },
                &session_id,
            ));
        }))
    }
}

/// Enriches a query with session context for better semantic matching.
fn enrich_query_with_session(query: &str, session: &Conversation) -> String {
    let mut parts = vec![query.to_string()];
    if let Some(ref title) = session.title {
        parts.push(title.clone());
    }
    if let Some(ref dir) = session.root_dir {
        // Use only the basename of the directory for conciseness
        if let Some(base) = std::path::Path::new(dir).file_name() {
            parts.push(base.to_string_lossy().to_string());
        }
    }
    parts.join(" ")
}

/// Extracts tags from session metadata for memory retrieval filtering.
fn extract_session_tags(session: &Conversation) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(ref lang) = session.language {
        tags.push(lang.to_lowercase());
    }
    tags
}

/// Returns the concatenated content of the N most recent user messages
/// (excluding the last one), each truncated to 200 chars.
fn recent_user_context(messages: &[Message], max_n: usize) -> String {
    let mut user_msgs = Vec::new();
    let mut skipped_last = false;

    for msg in messages.iter().rev() {
        if user_msgs.len() >= max_n {
            break;
        }
        if msg.role != "user" {
            continue;
        }
        // Skip the very last user message (already used as primary query)
        if !skipped_last {
            skipped_last = true;
            continue;
        }
        let content = prompt::truncate_utf8(&msg.content, 200);
        user_msgs.push(content.to_string());
    }

    user_msgs.join(" ")
}

/// Builds LLM-compatible tool definitions from Tool trait objects.
fn build_tool_definitions(tools: &[Arc<dyn Tool>]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|t| {
            let info = t.info();
            ToolDefinition {
                name: info.name,
                description: info.description,
                parameters: info.parameters,
            }
        })
        .collect()
}

/// Estimates the token overhead of tool definitions (JSON schema serialization).
fn estimate_tool_tokens(tool_defs: &[ToolDefinition]) -> usize {
    tool_defs
        .iter()
        .map(|td| {
            let name_tokens = estimate_tokens(&td.name);
            let desc_tokens = estimate_tokens(&td.description);
            let params_tokens = serde_json::to_string(&td.parameters)
                .map(|s| estimate_tokens(&s))
                .unwrap_or(0);
            name_tokens + desc_tokens + params_tokens
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::InMemoryConversationStore;
    use ozzie_core::events::Bus;
    use ozzie_llm::{ChatResponse, LlmError, TokenUsage, ToolDefinition};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockProvider {
        response: String,
        call_count: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatResponse, LlmError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(ChatResponse {
                content: self.response.clone(),
                tool_calls: Vec::new(),
                usage: TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            })
        }

        async fn chat_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<
            std::pin::Pin<
                Box<dyn futures_core::Stream<Item = Result<ChatDelta, LlmError>> + Send>,
            >,
            LlmError,
        > {
            // Return error to fall back to buffered
            Err(LlmError::Other("no stream".to_string()))
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    #[tokio::test]
    async fn processes_user_message() {
        let bus = Arc::new(Bus::new(64));
        let sessions = Arc::new(InMemoryConversationStore::new());
        let provider = Arc::new(MockProvider {
            response: "Hello!".to_string(),
            call_count: AtomicUsize::new(0),
        });

        // Create a session first
        let session = Conversation::new("sess_test");
        sessions.create(&session).await.unwrap();

        // Subscribe to assistant events before starting
        let mut rx = bus.subscribe(&[EventKind::AssistantMessage.as_str()]);

        let runner = Arc::new(EventRunner::new(
            bus.clone(),
            sessions.clone(),
            provider.clone(),
            "You are helpful.".to_string(),
        ));
        runner.start();

        // Give the runner time to subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Publish a user message
        bus.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::user_message("hi"),
            "sess_test",
        ));

        // Wait for assistant response
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for response")
            .expect("recv error");

        assert_eq!(response.event_type(), "assistant.message");
        let content = match &response.payload {
            EventPayload::AssistantMessage { content, .. } => content.as_str(),
            _ => "",
        };
        assert_eq!(content, "Hello!");

        // Verify messages were persisted
        let messages = sessions.load_messages("sess_test").await.unwrap();
        assert_eq!(messages.len(), 2); // user + assistant
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[1].role, "assistant");
    }

    #[tokio::test]
    async fn processes_with_config() {
        let bus = Arc::new(Bus::new(64));
        let sessions = Arc::new(InMemoryConversationStore::new());
        let provider = Arc::new(MockProvider {
            response: "Bonjour!".to_string(),
            call_count: AtomicUsize::new(0),
        });

        let mut session = Conversation::new("sess_config");
        session.root_dir = Some("/tmp/test".to_string());
        session.language = Some("fr".to_string());
        session.title = Some("Test Conversation".to_string());
        sessions.create(&session).await.unwrap();

        let mut rx = bus.subscribe(&[EventKind::AssistantMessage.as_str()]);

        let mut skill_descs = HashMap::new();
        skill_descs.insert("deploy".to_string(), "Deploy to production".to_string());

        let runner = Arc::new(EventRunner::with_config(EventRunnerConfig {
            bus: bus.clone(),
            sessions: sessions.clone() as Arc<dyn ConversationStore>,
            provider: provider.clone(),
            persona: "You are Ozzie.".to_string(),
            agent_instructions: "Be helpful.".to_string(),
            preferred_language: Some("fr".to_string()),
            skill_descriptions: skill_descs,
            custom_instructions: None,
            tools: Vec::new(),
            retriever: None,
            compressor: None,
            permissions: None,
            approver: None,
            dangerous_tool_names: Vec::new(),
            pairing_manager: None,
            actor_infos: Vec::new(),
            pool: None,
            provider_name: None,
            context_window: None,
            user_profile: None,
            blob_store: None,
            project_registry: None,
        }));
        runner.start();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        bus.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::user_message("salut"),
            "sess_config",
        ));

        let response = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout")
            .expect("recv error");

        assert_eq!(response.event_type(), "assistant.message");
        let content = match &response.payload {
            EventPayload::AssistantMessage { content, .. } => content.as_str(),
            _ => "",
        };
        assert_eq!(content, "Bonjour!");
    }

    #[test]
    fn enrich_query_basic() {
        let mut session = Conversation::new("s1");
        session.root_dir = Some("/home/user/projects/ozzie".to_string());
        session.language = Some("fr".to_string());
        session.title = Some("Debug Conversation".to_string());
        let enriched = enrich_query_with_session("how do I deploy?", &session);
        assert!(enriched.contains("how do I deploy?"));
        assert!(enriched.contains("Debug Conversation"));
        assert!(enriched.contains("ozzie")); // basename only
        assert!(!enriched.contains("/home/user")); // no full path
    }

    #[test]
    fn enrich_query_minimal_session() {
        let session = Conversation::new("s2");
        let enriched = enrich_query_with_session("hello", &session);
        assert_eq!(enriched, "hello");
    }

    #[test]
    fn extract_tags_from_session() {
        let mut session = Conversation::new("s3");
        session.language = Some("FR".to_string());
        let tags = extract_session_tags(&session);
        assert_eq!(tags, vec!["fr"]);
    }

    #[test]
    fn extract_tags_empty() {
        let session = Conversation::new("s4");
        let tags = extract_session_tags(&session);
        assert!(tags.is_empty());
    }

    #[test]
    fn recent_context_skips_last_user() {
        let msgs = vec![
            Message::user("first question"),
            Message::assistant("first answer"),
            Message::user("second question"),
            Message::assistant("second answer"),
            Message::user("current question"),
        ];
        let ctx = recent_user_context(&msgs, 2);
        assert!(ctx.contains("second question"));
        assert!(ctx.contains("first question"));
        assert!(!ctx.contains("current question"));
    }

    #[test]
    fn recent_context_truncates() {
        let long_msg = "x".repeat(300);
        let msgs = vec![
            Message::user(&long_msg),
            Message::assistant("ok"),
            Message::user("current"),
        ];
        let ctx = recent_user_context(&msgs, 2);
        assert!(ctx.len() <= 200);
        assert!(!ctx.contains(&"x".repeat(300)));
    }

    #[test]
    fn recent_context_empty_history() {
        let msgs = vec![Message::user("only message")];
        let ctx = recent_user_context(&msgs, 2);
        assert!(ctx.is_empty());
    }

    #[test]
    fn compose_prompt_with_memories() {
        let runner = EventRunner::new(
            Arc::new(Bus::new(64)),
            Arc::new(InMemoryConversationStore::new()),
            Arc::new(MockProvider {
                response: String::new(),
                call_count: AtomicUsize::new(0),
            }),
            "You are helpful.".to_string(),
        );

        let mut session = Conversation::new("s5");
        session.root_dir = Some("/tmp/test".to_string());
        session.language = Some("fr".to_string());

        let memories = vec![MemoryInfo {
            memory_type: "fact".to_string(),
            title: "Project".to_string(),
            content: "Ozzie is an agent OS.".to_string(),
        }];

        let prompt = runner.compose_system_prompt(&session, 5, &memories);
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("Conversation Context"));
        assert!(prompt.contains("Relevant Memories"));
        assert!(prompt.contains("**[fact] Project**"));
    }

    #[test]
    fn compose_prompt_without_memories() {
        let runner = EventRunner::new(
            Arc::new(Bus::new(64)),
            Arc::new(InMemoryConversationStore::new()),
            Arc::new(MockProvider {
                response: String::new(),
                call_count: AtomicUsize::new(0),
            }),
            "You are helpful.".to_string(),
        );

        let session = Conversation::new("s6");

        let prompt = runner.compose_system_prompt(&session, 0, &[]);
        assert_eq!(prompt, "You are helpful.");
        assert!(!prompt.contains("Memories"));
    }
}
