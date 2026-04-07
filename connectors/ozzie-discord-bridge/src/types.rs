use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Connector configuration as deserialized from `config.connectors.discord`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConnectorConfig {
    /// Set to false to temporarily disable the connector without removing config.
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// Bot token — use `"${{ .Env.DISCORD_BOT_TOKEN }}"` to load from environment.
    pub token: String,
}

impl Default for DiscordConnectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            token: String::new(),
        }
    }
}

/// Runtime database for the Discord connector.
///
/// Stored at `$OZZIE_PATH/connectors/discord.jsonc`.
/// Written at runtime by connector commands (`/init`, `/pair`, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscordDatabase {
    /// Direct user grants: maps Discord user ID → Ozzie policy name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub users: HashMap<String, String>,
    /// Per-guild configuration, keyed by guild ID (Discord snowflake as string).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub guilds: HashMap<String, DiscordGuildConfig>,
}

/// Configuration specific to one Discord guild (server).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscordGuildConfig {
    /// Channel ID where Ozzie sends admin notifications for this guild.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admin_channel: Option<String>,
    /// Maps Discord role IDs to Ozzie policy names.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub role_policies: HashMap<String, String>,
    /// Per-channel configuration, keyed by Discord channel ID.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub channels: HashMap<String, ChannelConfig>,
}

/// Per-channel Discord configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Semantic role of this channel.
    #[serde(default)]
    pub kind: ChannelKind,
    /// When Ozzie responds in this channel.
    #[serde(default)]
    pub respond_mode: RespondMode,
}

/// Semantic role of a Discord channel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    #[default]
    General,
    News,
    Support,
    Admin,
}

/// Controls when Ozzie responds in a Discord channel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RespondMode {
    /// Respond to every message.
    #[default]
    AllMessage,
    /// Respond only when the bot is @mentioned.
    WithMention,
}

fn bool_true() -> bool {
    true
}
