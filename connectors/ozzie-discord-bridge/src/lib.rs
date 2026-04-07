mod types;

pub use types::{
    ChannelConfig, ChannelKind, DiscordConnectorConfig, DiscordDatabase, DiscordGuildConfig,
    RespondMode,
};

use std::collections::HashMap;
use std::sync::Arc;

use ozzie_client::{EventKind, OzzieClient, OpenSessionOpts, PromptResponseParams};
use serenity::all::{
    ChannelId, Command, CommandDataOptionValue, CommandInteraction, CommandOptionType,
    ComponentInteraction, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, EditRole,
    EventHandler, GatewayIntents, Guild, GuildId, Interaction, Message, MessageId,
    Ready, RoleId, UserId,
};
use serenity::Client;
use tokio::sync::{mpsc, Mutex, Notify};
use tracing::{debug, info, warn};

// Re-export for CLI use
pub use ozzie_types::Reaction;

/// Basic bot identity returned by [`verify_token`].
pub struct BotInfo {
    pub id: String,
    pub display_name: String,
}

/// Validates a Discord bot token and returns the bot's identity.
pub async fn verify_token(token: &str) -> Result<BotInfo, String> {
    let http = serenity::http::Http::new(token);
    let user = http
        .get_current_user()
        .await
        .map_err(|e| format!("Invalid Discord token: {e}"))?;

    let display_name = match user.discriminator {
        Some(d) => format!("{}#{d:04}", user.name),
        None => user.name.clone(),
    };

    Ok(BotInfo {
        id: user.id.to_string(),
        display_name,
    })
}

/// A Discord message routed to the gateway.
#[derive(Debug)]
struct InboundMessage {
    channel_id: String,
    message_id: String,
    author: String,
    content: String,
}

/// A response from the gateway to send back to Discord.
#[derive(Debug)]
struct OutboundMessage {
    channel_id: String,
    content: String,
    reply_to_id: Option<String>,
}

/// Discord connector bridge — serenity bot + JSON-RPC gateway client.
///
/// Runs as a standalone bridge process. Discord messages are forwarded to the
/// Ozzie gateway via JSON-RPC WebSocket; responses are sent back to Discord.
pub struct DiscordBridge {
    token: String,
    db: Arc<Mutex<DiscordDatabase>>,
    db_path: Option<String>,
}

impl DiscordBridge {
    pub fn new(token: String, db_path: Option<String>) -> Result<Self, String> {
        if token.is_empty() {
            return Err("discord: token is required".to_string());
        }

        // Load DB from file if path provided
        let db = if let Some(ref path) = db_path {
            match std::fs::read_to_string(path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => DiscordDatabase::default(),
            }
        } else {
            DiscordDatabase::default()
        };

        Ok(Self {
            token,
            db: Arc::new(Mutex::new(db)),
            db_path,
        })
    }

    /// Creates a bridge from `OZZIE_CONNECTOR_CONFIG` environment variable.
    ///
    /// Expected JSON: `{"db_path": "...", "token": "..."}`
    /// The token can also come from `DISCORD_BOT_TOKEN` env var.
    /// Used when launched by the ProcessSupervisor.
    pub fn from_env() -> Result<Self, String> {
        let json = std::env::var("OZZIE_CONNECTOR_CONFIG")
            .map_err(|_| "OZZIE_CONNECTOR_CONFIG not set".to_string())?;
        let cfg: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("invalid OZZIE_CONNECTOR_CONFIG: {e}"))?;

        let token = cfg
            .get("token")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok())
            .ok_or("discord: token required in OZZIE_CONNECTOR_CONFIG or DISCORD_BOT_TOKEN env")?;

        let db_path = cfg
            .get("db_path")
            .and_then(|v| v.as_str())
            .map(String::from);

        Self::new(token, db_path)
    }

    /// Runs the bridge. Connects to both Discord (serenity) and the Ozzie gateway (JSON-RPC).
    ///
    /// Falls back to `OZZIE_GATEWAY_URL` and `OZZIE_GATEWAY_TOKEN` env vars
    /// when the corresponding arguments are not provided.
    pub async fn run(&self, gateway_url: &str, gateway_token: Option<&str>) -> Result<(), String> {
        // Resolve gateway URL/token from env vars if not provided
        let gateway_url = if gateway_url.is_empty() {
            std::env::var("OZZIE_GATEWAY_URL")
                .unwrap_or_else(|_| "ws://127.0.0.1:18420/ws".to_string())
        } else {
            gateway_url.to_string()
        };
        let token_env = std::env::var("OZZIE_GATEWAY_TOKEN").ok();
        let gateway_token: Option<String> = gateway_token.map(String::from).or(token_env);

        // Channel for Discord → gateway messages
        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<InboundMessage>();

        // Channel for gateway → Discord responses
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<OutboundMessage>();

        let shutdown = Arc::new(Notify::new());

        // Start serenity bot
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let bot_id = Arc::new(Mutex::new(None::<UserId>));

        let mut client = Client::builder(&self.token, intents)
            .event_handler(DiscordEventHandler {
                inbound_tx,
                db: self.db.clone(),
                db_path: self.db_path.clone(),
                bot_id: bot_id.clone(),
            })
            .await
            .map_err(|e| format!("discord client: {e}"))?;

        let http = client.http.clone();
        let shutdown2 = shutdown.clone();

        // Spawn serenity in background
        let shard_manager = client.shard_manager.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = client.start() => {
                    if let Err(e) = result {
                        tracing::error!(error = %e, "discord client error");
                    }
                }
                _ = shutdown2.notified() => {
                    shard_manager.shutdown_all().await;
                }
            }
        });

        info!("discord bridge: connecting to gateway {gateway_url}");

        // Connect to Ozzie gateway
        let mut ozzie = OzzieClient::connect(&gateway_url, gateway_token.as_deref())
            .await
            .map_err(|e| format!("gateway connect: {e}"))?;

        // Per-channel session mapping
        let mut channel_sessions: HashMap<String, String> = HashMap::new();

        // Spawn outbound writer (gateway → Discord)
        let http2 = http.clone();
        tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                let channel_id: u64 = match msg.channel_id.parse() {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                let mut builder = CreateMessage::new().content(&msg.content);
                if let Some(ref reply_id) = msg.reply_to_id
                    && let Ok(id) = reply_id.parse::<u64>()
                {
                    builder = builder.reference_message(serenity::all::MessageReference::from((
                        ChannelId::new(channel_id),
                        MessageId::new(id),
                    )));
                }

                if let Err(e) = ChannelId::new(channel_id).send_message(&http2, builder).await {
                    warn!(error = %e, "discord send failed");
                }
            }
        });

        info!("discord bridge: running");

        // Main loop: route messages between Discord and gateway
        loop {
            tokio::select! {
                // Discord → Gateway
                Some(msg) = inbound_rx.recv() => {
                    // Get or create session for this channel
                    let session_id = if let Some(sid) = channel_sessions.get(&msg.channel_id) {
                        sid.clone()
                    } else {
                        match ozzie.open_session(OpenSessionOpts {
                            session_id: None,
                            working_dir: None,
                        }).await {
                            Ok(sid) => {
                                // Accept all tools for connector sessions
                                if let Err(e) = ozzie.accept_all_tools().await {
                                    warn!(error = %e, "failed to accept all tools");
                                }
                                channel_sessions.insert(msg.channel_id.clone(), sid.clone());
                                sid
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to open session for channel {}", msg.channel_id);
                                continue;
                            }
                        }
                    };

                    debug!(channel = %msg.channel_id, session = %session_id, "forwarding to gateway");

                    if let Err(e) = ozzie.send_connector_message(
                        "discord",
                        &msg.channel_id,
                        &msg.author,
                        &msg.content,
                        Some(msg.message_id.as_str()).filter(|s| !s.is_empty()),
                    ).await {
                        warn!(error = %e, "send_connector_message failed");
                        continue;
                    }

                    // Wait for response and route back to Discord
                    let channel_id = msg.channel_id.clone();
                    let message_id = msg.message_id.clone();
                    match self.wait_for_response(&mut ozzie).await {
                        Ok(content) => {
                            if let Err(e) = outbound_tx.send(OutboundMessage {
                                channel_id,
                                content,
                                reply_to_id: Some(message_id),
                            }) {
                                warn!(error = %e, "outbound channel closed");
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "response error");
                            if let Err(e) = outbound_tx.send(OutboundMessage {
                                channel_id,
                                content: format!("⚠️ Error: {e}"),
                                reply_to_id: Some(message_id),
                            }) {
                                warn!(error = %e, "outbound channel closed");
                            }
                        }
                    }
                }
                else => break,
            }
        }

        shutdown.notify_one();
        Ok(())
    }

    async fn wait_for_response(&self, client: &mut OzzieClient) -> Result<String, String> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("timeout".to_string());
            }

            match tokio::time::timeout(remaining, client.read_frame()).await {
                Ok(Ok(frame)) => {
                    if !frame.is_notification() {
                        continue;
                    }

                    match frame.event_kind() {
                        Some(EventKind::AssistantMessage) => {
                            let content = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("content"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            return Ok(content);
                        }
                        Some(EventKind::PromptRequest) => {
                            if let Some(token) = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("token"))
                                .and_then(|v| v.as_str())
                                && let Err(e) = client
                                    .respond_to_prompt(PromptResponseParams {
                                        token: token.to_string(),
                                        value: Some("session".to_string()),
                                        text: None,
                                    })
                                    .await
                            {
                                warn!(error = %e, "failed to respond to prompt");
                            }
                        }
                        Some(EventKind::Error) => {
                            let msg = frame
                                .params
                                .as_ref()
                                .and_then(|p| p.get("message"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error");
                            return Err(msg.to_string());
                        }
                        _ => {}
                    }
                }
                Ok(Err(ozzie_client::ClientError::Closed)) => {
                    return Err("connection closed".to_string());
                }
                Ok(Err(e)) => return Err(format!("read error: {e}")),
                Err(_) => return Err("timeout".to_string()),
            }
        }
    }
}

// ---- Serenity event handler ----

struct DiscordEventHandler {
    inbound_tx: mpsc::UnboundedSender<InboundMessage>,
    db: Arc<Mutex<DiscordDatabase>>,
    db_path: Option<String>,
    bot_id: Arc<Mutex<Option<UserId>>>,
}

#[async_trait::async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, guilds = ready.guilds.len(), "discord bot connected");
        *self.bot_id.lock().await = Some(ready.user.id);

        if let Err(e) = Command::set_global_commands(&ctx.http, vec![]).await {
            warn!(error = %e, "failed to clear global commands");
        }

        for guild_status in &ready.guilds {
            self.register_guild_commands(&ctx, guild_status.id).await;
        }
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        if is_new == Some(true) {
            info!(guild = %guild.id, name = %guild.name, "bot joined new guild");
            self.register_guild_commands(&ctx, guild.id).await;
        }
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        // Apply channel respond-mode filter
        let channel_id_str = msg.channel_id.to_string();
        if let Some(guild_id) = msg.guild_id
            && let Some(guild) = self.db.lock().await.guilds.get(&guild_id.to_string()).cloned()
            && let Some(ch_cfg) = guild.channels.get(&channel_id_str)
            && ch_cfg.respond_mode == RespondMode::WithMention
        {
            let bot_id = *self.bot_id.lock().await;
            let mentioned = bot_id
                .map(|id| msg.mentions.iter().any(|u| u.id == id))
                .unwrap_or(false);
            if !mentioned {
                return;
            }
        }

        if let Err(e) = self.inbound_tx.send(InboundMessage {
            channel_id: msg.channel_id.to_string(),
            message_id: msg.id.to_string(),
            author: msg.author.name.clone(),
            content: msg.content.clone(),
        }) {
            warn!(error = %e, "inbound channel closed");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Component(component) => {
                self.handle_component_interaction(&ctx, component).await;
            }
            Interaction::Command(cmd) => {
                match cmd.data.name.as_str() {
                    "init" => self.handle_init_cmd(&ctx, &cmd).await,
                    "channels" => self.handle_channels_interaction(&ctx, &cmd).await,
                    _command_name => {
                        // For other slash commands, defer and route as message
                        if let Err(e) = cmd
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Defer(
                                    CreateInteractionResponseMessage::new(),
                                ),
                            )
                            .await
                        {
                            warn!(error = %e, "failed to defer slash command response");
                        }

                        if let Err(e) = self.inbound_tx.send(InboundMessage {
                            channel_id: cmd.channel_id.to_string(),
                            message_id: String::new(),
                            author: cmd.user.name.clone(),
                            content: format!("/{_command_name}"),
                        }) {
                            warn!(error = %e, "inbound channel closed");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ---- Command registration ----

fn build_commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("pair").description("Request pairing with Ozzie"),
        CreateCommand::new("status").description("Check your Ozzie pairing status"),
        CreateCommand::new("init")
            .description("Set up Ozzie roles in this server (guild admins only)"),
        CreateCommand::new("clear")
            .description("Start a new conversation (keeps history in logs)"),
        CreateCommand::new("channels")
            .description("Configure how Ozzie responds in a channel (guild admins only)")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "mode",
                    "Response mode or channel type",
                )
                .required(true)
                .add_string_choice("has-news", "has-news")
                .add_string_choice("has-support", "has-support")
                .add_string_choice("has-admin", "has-admin")
                .add_string_choice("with-mention", "with-mention")
                .add_string_choice("all-message", "all-message"),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::Channel, "channel", "Target channel")
                    .required(true),
            ),
    ]
}

// ---- Admin command handlers ----

impl DiscordEventHandler {
    async fn register_guild_commands(&self, ctx: &Context, guild_id: GuildId) {
        match guild_id.set_commands(&ctx.http, build_commands()).await {
            Ok(cmds) => info!(guild = %guild_id, count = cmds.len(), "guild commands registered"),
            Err(e) => warn!(guild = %guild_id, error = %e, "failed to register guild commands"),
        }
    }

    async fn handle_component_interaction(&self, ctx: &Context, component: ComponentInteraction) {
        let rating = match component.data.custom_id.as_str() {
            "feedback_positive" => "positive",
            "feedback_negative" => "negative",
            _ => return,
        };

        info!(
            channel = %component.channel_id,
            message = %component.message.id,
            user = %component.user.id,
            rating,
            "feedback received"
        );

        if let Err(e) = component
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new().components(vec![]),
                ),
            )
            .await
        {
            warn!(error = %e, "failed to update feedback message");
        }
    }

    async fn handle_init_cmd(&self, ctx: &Context, cmd: &CommandInteraction) {
        let Some(guild_id) = cmd.guild_id else {
            if let Err(e) = cmd
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("⚠️ `/init` must be used in a server.")
                            .ephemeral(true),
                    ),
                )
                .await
            {
                warn!(error = %e, "failed to send init guild-only response");
            }
            return;
        };

        if let Err(e) = cmd
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
            )
            .await
        {
            warn!(error = %e, "failed to defer init response");
        }

        let existing_roles: HashMap<String, RoleId> =
            match ctx.http.get_guild_roles(guild_id).await {
                Ok(roles) => roles.into_iter().map(|r| (r.name.clone(), r.id)).collect(),
                Err(e) => {
                    warn!(error = %e, "failed to fetch guild roles");
                    HashMap::new()
                }
            };

        let role_defs = [("ozzie-admin", "admin"), ("ozzie-support", "support")];
        let mut created: Vec<String> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();

        for (role_name, policy) in &role_defs {
            if let Some(&role_id) = existing_roles.get(*role_name) {
                let mut db = self.db.lock().await;
                db.guilds
                    .entry(guild_id.to_string())
                    .or_default()
                    .role_policies
                    .insert(role_id.to_string(), policy.to_string());
                self.save_db(&db);
                skipped.push(format!("`{role_name}`"));
            } else {
                match guild_id
                    .create_role(&ctx.http, EditRole::new().name(*role_name))
                    .await
                {
                    Ok(role) => {
                        let mut db = self.db.lock().await;
                        db.guilds
                            .entry(guild_id.to_string())
                            .or_default()
                            .role_policies
                            .insert(role.id.to_string(), policy.to_string());
                        self.save_db(&db);
                        created.push(format!("`{role_name}` → `{policy}`"));
                    }
                    Err(e) => warn!(role = role_name, error = %e, "failed to create role"),
                }
            }
        }

        let mut lines = Vec::new();
        if !created.is_empty() {
            lines.push(format!("Roles created: {}", created.join(", ")));
        }
        if !skipped.is_empty() {
            lines.push(format!("Already existed: {}", skipped.join(", ")));
        }
        lines.push("Assign these roles to your members to control access.".to_string());

        if let Err(e) = cmd
            .edit_response(
                &ctx.http,
                serenity::all::EditInteractionResponse::new().content(lines.join("\n")),
            )
            .await
        {
            warn!(error = %e, "failed to edit init response");
        }
    }

    async fn handle_channels_interaction(&self, ctx: &Context, cmd: &CommandInteraction) {
        let Some(guild_id) = cmd.guild_id else {
            if let Err(e) = cmd
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("⚠️ `/channels` must be used in a server.")
                            .ephemeral(true),
                    ),
                )
                .await
            {
                warn!(error = %e, "failed to send channels guild-only response");
            }
            return;
        };

        let mode = cmd.data.options.iter().find(|o| o.name == "mode").and_then(|o| {
            if let CommandDataOptionValue::String(s) = &o.value { Some(s.clone()) } else { None }
        });

        let channel = cmd.data.options.iter().find(|o| o.name == "channel").and_then(|o| {
            if let CommandDataOptionValue::Channel(id) = &o.value { Some(*id) } else { None }
        });

        let (Some(mode), Some(channel_id)) = (mode, channel) else {
            if let Err(e) = cmd
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("⚠️ Missing required options.")
                            .ephemeral(true),
                    ),
                )
                .await
            {
                warn!(error = %e, "failed to send missing options response");
            }
            return;
        };

        let content = match self.apply_channel_mode(guild_id.to_string(), channel_id.to_string(), &mode).await {
            Ok(()) => format!("Channel <#{}> configured as `{mode}`.", channel_id),
            Err(e) => format!("⚠️ {e}"),
        };

        if let Err(e) = cmd
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content)
                        .ephemeral(true),
                ),
            )
            .await
        {
            warn!(error = %e, "failed to send channels response");
        }
    }

    async fn apply_channel_mode(&self, guild_id: String, channel_id: String, mode: &str) -> Result<(), String> {
        let mut db = self.db.lock().await;
        let ch = db.guilds
            .entry(guild_id)
            .or_default()
            .channels
            .entry(channel_id)
            .or_default();

        match mode {
            "has-news" => ch.kind = ChannelKind::News,
            "has-support" => ch.kind = ChannelKind::Support,
            "has-admin" => ch.kind = ChannelKind::Admin,
            "with-mention" => ch.respond_mode = RespondMode::WithMention,
            "all-message" => ch.respond_mode = RespondMode::AllMessage,
            other => return Err(format!("Unknown mode `{other}`")),
        }

        self.save_db(&db);
        Ok(())
    }

    fn save_db(&self, db: &DiscordDatabase) {
        if let Some(ref path) = self.db_path {
            match serde_json::to_string_pretty(db) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(path, json) {
                        warn!(error = %e, path, "failed to write discord db");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "failed to serialize discord db");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_channel_ref(s: &str) -> Option<ChannelId> {
        let id_str = if s.starts_with("<#") && s.ends_with('>') {
            &s[2..s.len() - 1]
        } else {
            s
        };
        id_str.parse::<u64>().ok().map(ChannelId::new)
    }

    #[test]
    fn parse_channel_ref_mention() {
        let ch = parse_channel_ref("<#123456789012345678>");
        assert_eq!(ch.map(|c| c.get()), Some(123456789012345678));
    }

    #[test]
    fn parse_channel_ref_bare_id() {
        let ch = parse_channel_ref("987654321");
        assert_eq!(ch.map(|c| c.get()), Some(987654321));
    }

    #[test]
    fn parse_channel_ref_invalid() {
        assert!(parse_channel_ref("not-an-id").is_none());
    }
}
