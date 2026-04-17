use serde::{Deserialize, Serialize};

/// Connector configuration for the Discord bridge.
///
/// Read from `OZZIE_CONNECTOR_CONFIG` JSON or individual env vars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConnectorConfig {
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// Discord bot token.
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

fn bool_true() -> bool {
    true
}
