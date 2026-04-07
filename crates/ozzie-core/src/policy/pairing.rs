use serde::{Deserialize, Serialize};

/// Identifies a user in a platform context. Wildcards: "*" matches any value.
///
/// Intentionally excludes channel_id — pairing is per-user, not per-channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingKey {
    pub platform: String,
    pub server_id: String,
    pub user_id: String,
}

/// Maps a user×context to a policy name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub key: PairingKey,
    pub policy_name: String,
}

// JsonPairingStore has been moved to ozzie-runtime::json_pairing_store.
