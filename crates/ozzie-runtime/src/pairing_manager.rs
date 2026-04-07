use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use ozzie_core::connector::{Identity, IncomingMessage};
use ozzie_core::domain::{
    DeviceRecord, DeviceStorage, PairingError, PairingStorage, PendingKind, PendingPairings,
    PendingRequest,
};
use ozzie_core::events::{Event, EventBus, EventPayload, EventSource};
use ozzie_utils::names;
use ozzie_core::policy::{Pairing, PairingKey};
use tracing::info;

/// Default TTL for pending pairing requests.
const REQUEST_TTL_MINUTES: i64 = 15;

/// Summary of a pending request for CLI/API display.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PendingRequestSummary {
    Device {
        request_id: String,
        client_type: Option<String>,
        label: Option<String>,
        requested_at: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
    },
    Chat {
        request_id: String,
        display_name: Option<String>,
        platform: Option<String>,
        requested_at: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
    },
}

impl PendingRequestSummary {
    /// Returns the request ID regardless of variant.
    pub fn request_id(&self) -> &str {
        match self {
            Self::Device { request_id, .. } | Self::Chat { request_id, .. } => request_id,
        }
    }

    /// Returns `true` if this is a device pairing request.
    pub fn is_device(&self) -> bool {
        matches!(self, Self::Device { .. })
    }

    /// Returns `true` if this is a chat pairing request.
    pub fn is_chat(&self) -> bool {
        matches!(self, Self::Chat { .. })
    }
}

/// Manages the full lifecycle of chat connector pairing requests.
///
/// - Receives "/pair" commands from IncomingMessages
/// - Publishes PairingRequest events for admin visibility
/// - Approves/rejects requests via CLI/API
/// - Persists approved pairings to `PairingStorage`
pub struct PairingManager {
    pending: Arc<dyn PendingPairings>,
    chat_storage: Arc<dyn PairingStorage>,
    bus: Arc<dyn EventBus>,
    /// Per-guild role-based policy fallback: guild_id → (role_id → policy_name).
    ///
    /// Discord role IDs are guild-specific snowflakes. When an identity has no
    /// explicit chat pairing, `resolve_policy` looks up the guild's role map
    /// using `identity.server_id` and checks if any of the user's roles match.
    guild_role_policies: HashMap<String, HashMap<String, String>>,
}

impl PairingManager {
    pub fn new(
        pending: Arc<dyn PendingPairings>,
        chat_storage: Arc<dyn PairingStorage>,
        bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            pending,
            chat_storage,
            bus,
            guild_role_policies: HashMap::new(),
        }
    }

    /// Creates a PairingManager with per-guild role → policy maps.
    ///
    /// `guild_role_policies` is keyed by guild ID (e.g. Discord server snowflake).
    /// Each value maps platform role IDs to Ozzie policy names.
    pub fn new_with_guild_roles(
        pending: Arc<dyn PendingPairings>,
        chat_storage: Arc<dyn PairingStorage>,
        bus: Arc<dyn EventBus>,
        guild_role_policies: HashMap<String, HashMap<String, String>>,
    ) -> Self {
        Self {
            pending,
            chat_storage,
            bus,
            guild_role_policies,
        }
    }

    /// Creates a device pairing request and publishes a PairingRequest event.
    /// Returns the generated request_id.
    pub fn create_device_request(&self, client_type: &str, label: Option<&str>) -> String {
        let request_id = names::generate_id("pair", |_| false);
        let now = Utc::now();
        let expires_at = now + Duration::minutes(REQUEST_TTL_MINUTES);

        let req = PendingRequest {
            request_id: request_id.clone(),
            kind: PendingKind::Device {
                client_type: client_type.to_string(),
                label: label.map(str::to_string),
            },
            requested_at: now,
            expires_at,
        };

        self.pending.insert(req);

        info!(
            request_id = %request_id,
            kind = "device",
            client_type,
            label = label.unwrap_or("-"),
            "pairing request created"
        );

        self.bus.publish(Event::new(
            EventSource::Connector,
            EventPayload::PairingRequestDevice {
                request_id: request_id.clone(),
                client_type: Some(client_type.to_string()),
                label: label.map(str::to_string),
            },
        ));

        request_id
    }

    /// Approves a pending device pairing request.
    ///
    /// The caller is responsible for generating `device_id` and `token`
    /// (use uuid v4 for both). The approved token is stored in `device_storage`
    /// for future WS authentication.
    pub fn approve_device(
        &self,
        request_id: &str,
        device_id: &str,
        token: &str,
        device_storage: &dyn DeviceStorage,
    ) -> Result<(), PairingError> {
        let req = self
            .pending
            .take(request_id)
            .ok_or_else(|| PairingError::NotFound(request_id.to_string()))?;

        let PendingKind::Device { client_type, label } = req.kind else {
            self.pending.insert(req);
            return Err(PairingError::NotFound(format!(
                "{request_id} is not a device request"
            )));
        };

        device_storage.add(DeviceRecord {
            device_id: device_id.to_string(),
            client_type: client_type.clone(),
            label: label.clone(),
            token: token.to_string(),
            paired_at: Utc::now(),
            last_seen: None,
        })?;

        info!(
            request_id,
            device_id,
            client_type = %client_type,
            label = label.as_deref().unwrap_or("-"),
            "device pairing approved"
        );

        self.bus.publish(Event::new(
            EventSource::Agent,
            EventPayload::PairingApprovedDevice {
                request_id: request_id.to_string(),
                approved_by: "cli".to_string(),
                device_id: Some(device_id.to_string()),
            },
        ));

        Ok(())
    }

    /// Called when an IncomingMessage has command == "pair".
    /// Returns the generated request_id.
    pub fn on_pair_request(&self, msg: &IncomingMessage) -> String {
        let request_id = names::generate_id("pair", |_| false);
        let now = Utc::now();
        let expires_at = now + Duration::minutes(REQUEST_TTL_MINUTES);

        let req = PendingRequest {
            request_id: request_id.clone(),
            kind: PendingKind::Chat {
                identity: msg.identity.clone(),
                display_name: msg.identity.name.clone(),
                platform_message: msg.content.clone(),
            },
            requested_at: now,
            expires_at,
        };

        self.pending.insert(req);

        info!(
            request_id = %request_id,
            kind = "chat",
            platform = %msg.identity.platform,
            user_id = %msg.identity.user_id,
            display_name = %msg.identity.name,
            server_id = %msg.identity.server_id,
            "pairing request created"
        );

        self.bus.publish(Event::new(
            EventSource::Connector,
            EventPayload::PairingRequestChat {
                request_id: request_id.clone(),
                platform: Some(msg.identity.platform.clone()),
                server_id: Some(msg.identity.server_id.clone()),
                channel_id: Some(msg.identity.channel_id.clone()),
                user_id: Some(msg.identity.user_id.clone()),
                display_name: Some(msg.identity.name.clone()),
            },
        ));

        request_id
    }

    /// Approves a pending chat pairing request.
    pub fn approve_chat(
        &self,
        request_id: &str,
        policy_name: &str,
        approved_by: &str,
    ) -> Result<(), PairingError> {
        let req = self
            .pending
            .take(request_id)
            .ok_or_else(|| PairingError::NotFound(request_id.to_string()))?;

        let PendingKind::Chat { identity, .. } = req.kind else {
            // Not a chat request — put it back
            self.pending.insert(req);
            return Err(PairingError::NotFound(format!(
                "{request_id} is not a chat request"
            )));
        };

        self.chat_storage.add(&Pairing {
            key: PairingKey {
                platform: identity.platform.clone(),
                server_id: identity.server_id.clone(),
                user_id: identity.user_id.clone(),
            },
            policy_name: policy_name.to_string(),
        })?;

        info!(
            request_id,
            kind = "chat",
            platform = %identity.platform,
            user_id = %identity.user_id,
            policy = policy_name,
            approved_by,
            "pairing approved"
        );

        self.bus.publish(Event::new(
            EventSource::Agent,
            EventPayload::PairingApprovedChat {
                request_id: request_id.to_string(),
                approved_by: approved_by.to_string(),
                policy_name: Some(policy_name.to_string()),
            },
        ));

        Ok(())
    }

    /// Registers a wildcard pairing for all identities on a platform.
    ///
    /// Used by connectors (e.g. the file connector) that define an `auto_pair_policy`
    /// in config. Creates a `platform/*/* → policy_name` entry so every identity
    /// from that platform is automatically served without a manual approval step.
    ///
    /// If a pairing for that platform already exists it is overwritten with the new policy.
    /// Does nothing if `policy_name` is empty.
    pub fn register_platform_pairing(
        &self,
        platform: &str,
        policy_name: &str,
    ) -> Result<(), PairingError> {
        if policy_name.is_empty() {
            return Ok(());
        }
        self.chat_storage.add(&Pairing {
            key: PairingKey {
                platform: platform.to_string(),
                server_id: "*".to_string(),
                user_id: "*".to_string(),
            },
            policy_name: policy_name.to_string(),
        })
    }

    /// Rejects a pending pairing request (device or chat).
    pub fn reject(&self, request_id: &str, rejected_by: &str) -> Result<(), PairingError> {
        self.pending
            .take(request_id)
            .ok_or_else(|| PairingError::NotFound(request_id.to_string()))?;

        info!(request_id, rejected_by, "pairing rejected");

        self.bus.publish(Event::new(
            EventSource::Agent,
            EventPayload::PairingRejected {
                request_id: request_id.to_string(),
                rejected_by: rejected_by.to_string(),
            },
        ));

        Ok(())
    }

    /// Lists all non-expired pending requests.
    pub fn list_pending(&self) -> Vec<PendingRequestSummary> {
        self.pending.purge_expired();
        self.pending
            .list()
            .into_iter()
            .map(|r| match &r.kind {
                PendingKind::Chat {
                    identity,
                    display_name,
                    ..
                } => PendingRequestSummary::Chat {
                    request_id: r.request_id,
                    display_name: Some(display_name.clone()),
                    platform: Some(identity.platform.clone()),
                    requested_at: r.requested_at,
                    expires_at: r.expires_at,
                },
                PendingKind::Device { client_type, label } => PendingRequestSummary::Device {
                    request_id: r.request_id,
                    client_type: Some(client_type.clone()),
                    label: label.clone(),
                    requested_at: r.requested_at,
                    expires_at: r.expires_at,
                },
            })
            .collect()
    }

    /// Resolves the policy for an identity.
    ///
    /// Priority:
    /// 1. Exact identity match in `chat_storage` (explicit `/pair` approval).
    /// 2. Guild-scoped role fallback: looks up `identity.server_id` in
    ///    `guild_role_policies`, then checks each role in `roles`.
    pub fn resolve_policy(&self, identity: &Identity, roles: &[String]) -> Option<String> {
        if let Some(policy) = self.chat_storage.resolve(identity) {
            return Some(policy);
        }
        if let Some(role_map) = self.guild_role_policies.get(&identity.server_id) {
            for role in roles {
                if let Some(policy) = role_map.get(role) {
                    return Some(policy.clone());
                }
            }
        }
        None
    }

    /// Direct access to chat pairings for listing.
    pub fn list_chat_pairings(&self) -> Vec<Pairing> {
        self.chat_storage.list()
    }

    /// Removes a specific chat pairing.
    pub fn remove_chat_pairing(&self, key: &PairingKey) -> Result<bool, PairingError> {
        self.chat_storage.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::connector::Identity;
    use ozzie_core::events::Bus;
    use ozzie_core::policy::MemoryPendingPairings;
    use crate::json_pairing_store::JsonPairingStore;

    fn make_msg(platform: &str, user_id: &str) -> IncomingMessage {
        IncomingMessage {
            identity: Identity {
                platform: platform.to_string(),
                user_id: user_id.to_string(),
                name: "Alice".to_string(),
                server_id: "s1".to_string(),
                channel_id: "dm".to_string(),
            },
            content: "/pair".to_string(),
            channel_id: "dm".to_string(),
            message_id: "msg_1".to_string(),
            timestamp: Utc::now(),
            command: Some("pair".to_string()),
            is_dm: true,
            ..Default::default()
        }
    }

    fn make_manager(dir: &std::path::Path) -> PairingManager {
        let pending = Arc::new(MemoryPendingPairings::new());
        let chat_storage = Arc::new(JsonPairingStore::new(dir));
        let bus = Arc::new(Bus::new(16));
        PairingManager::new(pending, chat_storage, bus)
    }

    #[test]
    fn on_pair_request_creates_pending() {
        let dir = tempfile::tempdir().unwrap();
        let pm = make_manager(dir.path());
        let msg = make_msg("discord", "u1");

        pm.on_pair_request(&msg);

        let pending = pm.list_pending();
        assert_eq!(pending.len(), 1);
        assert!(matches!(
            &pending[0],
            PendingRequestSummary::Chat { platform, .. } if platform.as_deref() == Some("discord")
        ));
    }

    #[test]
    fn approve_chat_stores_pairing() {
        let dir = tempfile::tempdir().unwrap();
        let pm = make_manager(dir.path());
        let msg = make_msg("discord", "u1");

        let request_id = pm.on_pair_request(&msg);
        pm.approve_chat(&request_id, "support", "admin").unwrap();

        assert_eq!(pm.list_chat_pairings().len(), 1);
        assert_eq!(pm.list_pending().len(), 0);
    }

    #[test]
    fn reject_removes_pending() {
        let dir = tempfile::tempdir().unwrap();
        let pm = make_manager(dir.path());
        let msg = make_msg("discord", "u1");

        let request_id = pm.on_pair_request(&msg);
        pm.reject(&request_id, "admin").unwrap();

        assert_eq!(pm.list_pending().len(), 0);
    }

    #[test]
    fn approve_unknown_request_errors() {
        let dir = tempfile::tempdir().unwrap();
        let pm = make_manager(dir.path());

        let result = pm.approve_chat("unknown_id", "support", "admin");
        assert!(matches!(result, Err(PairingError::NotFound(_))));
    }

    #[test]
    fn resolve_policy_guild_role_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let pending = Arc::new(MemoryPendingPairings::new());
        let chat_storage = Arc::new(JsonPairingStore::new(dir.path()));
        let bus = Arc::new(Bus::new(16));

        let mut guild_roles = HashMap::new();
        let mut role_map = HashMap::new();
        role_map.insert("role_admin".to_string(), "admin".to_string());
        role_map.insert("role_support".to_string(), "support".to_string());
        guild_roles.insert("guild_abc".to_string(), role_map);

        let pm = PairingManager::new_with_guild_roles(pending, chat_storage, bus, guild_roles);

        let identity = Identity {
            platform: "discord".to_string(),
            user_id: "u1".to_string(),
            name: "Alice".to_string(),
            server_id: "guild_abc".to_string(),
            channel_id: "ch1".to_string(),
        };

        // Role match in correct guild → resolved
        assert_eq!(
            pm.resolve_policy(&identity, &["role_support".to_string()]),
            Some("support".to_string())
        );

        // Unknown role → None
        assert_eq!(pm.resolve_policy(&identity, &["role_unknown".to_string()]), None);

        // Correct role but wrong guild → None
        let other_guild_identity = Identity {
            server_id: "guild_xyz".to_string(),
            ..identity.clone()
        };
        assert_eq!(
            pm.resolve_policy(&other_guild_identity, &["role_admin".to_string()]),
            None
        );
    }
}
