mod types;

pub use types::DiscordConnectorConfig;

use std::collections::HashMap;
use std::sync::Arc;

use ozzie_gateway_client::{
    ConnectorMessageParams, GatewayClient, GatewayError, Notification, OpenConversationOpts,
    PromptResponseParams, WsGatewayClient,
};
use serenity::all::{
    ChannelId, Command, Context, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, EventHandler, GatewayIntents, Interaction,
    Message, MessageId, Ready,
};
use serenity::Client;
use tokio::sync::{mpsc, Notify};
use tracing::{debug, info, warn};

const CONNECTOR_NAME: &str = "discord";
const DEFAULT_GATEWAY_URL: &str = "ws://127.0.0.1:18420/ws";
const RESPONSE_TIMEOUT_SECS: u64 = 300;

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

// ---- Internal message types ----

#[derive(Debug)]
struct InboundMessage {
    channel_id: String,
    message_id: String,
    author_id: String,
    content: String,
}

#[derive(Debug)]
struct OutboundMessage {
    channel_id: String,
    content: String,
    reply_to_id: Option<String>,
}

// ---- Bridge ----

/// Discord connector bridge — forwards messages to the Ozzie gateway.
///
/// Access control is delegated to the gateway's pairing system.
/// The bridge forwards all non-bot messages; unpaired users receive
/// a hint to use `/pair`.
pub struct DiscordBridge {
    token: String,
}

impl DiscordBridge {
    pub fn new(token: String) -> Result<Self, String> {
        if token.is_empty() {
            return Err("discord: bot token is required".to_string());
        }
        Ok(Self { token })
    }

    /// Creates a bridge from environment variables.
    ///
    /// Reads `OZZIE_CONNECTOR_CONFIG` JSON (`{"token": "..."}`).
    /// Falls back to `DISCORD_BOT_TOKEN` env var.
    pub fn from_env() -> Result<Self, String> {
        let json = std::env::var("OZZIE_CONNECTOR_CONFIG")
            .map_err(|_| "OZZIE_CONNECTOR_CONFIG not set".to_string())?;
        let cfg: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("invalid config: {e}"))?;

        let token = cfg
            .get("token")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok())
            .ok_or("discord: token required in config or DISCORD_BOT_TOKEN env")?;

        Self::new(token)
    }

    /// Run the bridge. Connects to Discord and the Ozzie gateway.
    pub async fn run(&self, gateway_url: &str, gateway_token: Option<&str>) -> Result<(), String> {
        let gateway_url = if gateway_url.is_empty() {
            std::env::var("OZZIE_GATEWAY_URL")
                .unwrap_or_else(|_| DEFAULT_GATEWAY_URL.to_string())
        } else {
            gateway_url.to_string()
        };
        let token_env = std::env::var("OZZIE_GATEWAY_TOKEN").ok();
        let gateway_token: Option<String> = gateway_token.map(String::from).or(token_env);

        let (inbound_tx, mut inbound_rx) = mpsc::unbounded_channel::<InboundMessage>();
        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<OutboundMessage>();
        let shutdown = Arc::new(Notify::new());

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let mut client = Client::builder(&self.token, intents)
            .event_handler(DiscordEventHandler { inbound_tx })
            .await
            .map_err(|e| format!("discord client: {e}"))?;

        let http = client.http.clone();
        let shutdown2 = shutdown.clone();
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

        let mut gateway = WsGatewayClient::connect(&gateway_url, gateway_token.as_deref())
            .await
            .map_err(|e| format!("gateway connect: {e}"))?;

        let mut channel_sessions: HashMap<String, String> = HashMap::new();

        // Outbound writer: gateway responses → Discord messages
        let http2 = http.clone();
        tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                let Ok(channel_id) = msg.channel_id.parse::<u64>() else {
                    continue;
                };

                let mut builder = CreateMessage::new().content(&msg.content);
                if let Some(Ok(id)) = msg.reply_to_id.as_ref().map(|r| r.parse::<u64>()) {
                    builder =
                        builder.reference_message(serenity::all::MessageReference::from((
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

        // Main loop: messages → gateway → Discord reply
        loop {
            tokio::select! {
                Some(msg) = inbound_rx.recv() => {
                    let conversation_id = if let Some(sid) = channel_sessions.get(&msg.channel_id) {
                        sid.clone()
                    } else {
                        match gateway.open_session(OpenConversationOpts::default()).await {
                            Ok(info) => {
                                if let Err(e) = gateway.accept_all_tools().await {
                                    warn!(error = %e, "failed to accept all tools");
                                }
                                channel_sessions.insert(msg.channel_id.clone(), info.conversation_id.clone());
                                info.conversation_id
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to open session");
                                continue;
                            }
                        }
                    };

                    debug!(channel = %msg.channel_id, session = %conversation_id, "forwarding to gateway");

                    if let Err(e) = gateway
                        .send_connector_message(ConnectorMessageParams {
                            connector: CONNECTOR_NAME.into(),
                            channel_id: msg.channel_id.clone(),
                            author: msg.author_id.clone(),
                            content: msg.content.clone(),
                            message_id: (!msg.message_id.is_empty())
                                .then(|| msg.message_id.clone()),
                            server_id: None,
                        })
                        .await
                    {
                        warn!(error = %e, "send_connector_message failed");
                        continue;
                    }

                    let channel_id = msg.channel_id.clone();
                    let message_id = msg.message_id.clone();
                    match self.wait_for_response(&mut gateway).await {
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
                            let _ = outbound_tx.send(OutboundMessage {
                                channel_id,
                                content: format!("Error: {e}"),
                                reply_to_id: Some(message_id),
                            });
                        }
                    }
                }
                else => break,
            }
        }

        shutdown.notify_one();
        Ok(())
    }

    async fn wait_for_response(&self, gateway: &mut WsGatewayClient) -> Result<String, String> {
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_secs(RESPONSE_TIMEOUT_SECS);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("timeout".to_string());
            }

            match tokio::time::timeout(remaining, gateway.read_notification()).await {
                Ok(Ok(notification)) => match notification {
                    Notification::AssistantMessage(e) => return Ok(e.content),
                    Notification::ConnectorReply(e) if e.connector == CONNECTOR_NAME => {
                        return Ok(e.content);
                    }
                    Notification::PromptRequest(e) => {
                        if let Err(err) = gateway
                            .respond_to_prompt(PromptResponseParams {
                                token: e.token,
                                value: Some("session".to_string()),
                                text: None,
                            })
                            .await
                        {
                            warn!(error = %err, "failed to respond to prompt");
                        }
                    }
                    Notification::Error(e) => {
                        return Err(e.message.unwrap_or_else(|| "unknown error".into()));
                    }
                    _ => {}
                },
                Ok(Err(GatewayError::Closed)) => return Err("connection closed".to_string()),
                Ok(Err(e)) => return Err(format!("read error: {e}")),
                Err(_) => return Err("timeout".to_string()),
            }
        }
    }
}

// ---- Serenity event handler ----

struct DiscordEventHandler {
    inbound_tx: mpsc::UnboundedSender<InboundMessage>,
}

#[async_trait::async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(
            user = %ready.user.name,
            guilds = ready.guilds.len(),
            "discord bot connected"
        );

        match Command::set_global_commands(&ctx.http, build_commands()).await {
            Ok(cmds) => info!(count = cmds.len(), "global commands registered"),
            Err(e) => warn!(error = %e, "failed to register global commands"),
        }
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if let Err(e) = self.inbound_tx.send(InboundMessage {
            channel_id: msg.channel_id.to_string(),
            message_id: msg.id.to_string(),
            author_id: msg.author.id.to_string(),
            content: msg.content.clone(),
        }) {
            warn!(error = %e, "inbound channel closed");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(cmd) = interaction else {
            return;
        };

        match cmd.data.name.as_str() {
            "pair" | "clear" => {
                // Acknowledge immediately with ephemeral message so Discord
                // stops showing "thinking". The actual reply arrives as a
                // regular message from the gateway.
                if let Err(e) = cmd
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Forwarding to Ozzie…")
                                .ephemeral(true),
                        ),
                    )
                    .await
                {
                    warn!(error = %e, "failed to acknowledge command");
                }

                let _ = self.inbound_tx.send(InboundMessage {
                    channel_id: cmd.channel_id.to_string(),
                    message_id: String::new(),
                    author_id: cmd.user.id.to_string(),
                    content: format!("/{}", cmd.data.name),
                });
            }
            _ => {}
        }
    }
}

fn build_commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("pair").description("Request pairing with Ozzie"),
        CreateCommand::new("clear").description("Start a new conversation"),
    ]
}

#[cfg(test)]
mod tests {
    use serenity::all::ChannelId;

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
