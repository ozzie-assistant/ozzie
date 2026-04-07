use std::sync::Mutex;

use crate::domain::{PendingPairings, PendingRequest};

/// Default TTL for pending pairing requests.
const DEFAULT_TTL_SECS: u64 = 15 * 60; // 15 minutes

/// In-memory pending pairing requests store with TTL eviction.
///
/// Both device and chat pairing flows share this store.
/// Not persisted — requests expire after TTL and are re-created by the user.
pub struct MemoryPendingPairings {
    requests: Mutex<Vec<PendingRequest>>,
}

impl MemoryPendingPairings {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Default TTL duration for new requests.
    pub fn default_ttl() -> std::time::Duration {
        std::time::Duration::from_secs(DEFAULT_TTL_SECS)
    }
}

impl Default for MemoryPendingPairings {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingPairings for MemoryPendingPairings {
    fn insert(&self, req: PendingRequest) {
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        // Replace if same request_id (idempotent)
        requests.retain(|r| r.request_id != req.request_id);
        requests.push(req);
    }

    fn list(&self) -> Vec<PendingRequest> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn take(&self, request_id: &str) -> Option<PendingRequest> {
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        requests
            .iter()
            .position(|r| r.request_id == request_id)
            .map(|pos| requests.remove(pos))
    }

    fn purge_expired(&self) {
        let now = chrono::Utc::now();
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        requests.retain(|r| r.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use crate::connector::Identity;
    use crate::domain::PendingKind;

    fn device_request(id: &str, secs_from_now: i64) -> PendingRequest {
        PendingRequest {
            request_id: id.to_string(),
            kind: PendingKind::Device {
                client_type: "tui".to_string(),
                label: None,
            },
            requested_at: Utc::now(),
            expires_at: Utc::now() + Duration::seconds(secs_from_now),
        }
    }

    fn chat_request(id: &str, secs_from_now: i64) -> PendingRequest {
        PendingRequest {
            request_id: id.to_string(),
            kind: PendingKind::Chat {
                identity: Identity {
                    platform: "discord".to_string(),
                    user_id: "u1".to_string(),
                    name: "Alice".to_string(),
                    server_id: "s1".to_string(),
                    channel_id: "dm".to_string(),
                },
                display_name: "Alice".to_string(),
                platform_message: "/pair".to_string(),
            },
            requested_at: Utc::now(),
            expires_at: Utc::now() + Duration::seconds(secs_from_now),
        }
    }

    #[test]
    fn insert_and_list() {
        let store = MemoryPendingPairings::new();
        store.insert(device_request("req_1", 900));
        store.insert(chat_request("req_2", 900));
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn take_consumes() {
        let store = MemoryPendingPairings::new();
        store.insert(device_request("req_1", 900));
        let taken = store.take("req_1").unwrap();
        assert_eq!(taken.request_id, "req_1");
        assert!(store.take("req_1").is_none()); // already consumed
        assert_eq!(store.list().len(), 0);
    }

    #[test]
    fn purge_removes_expired() {
        let store = MemoryPendingPairings::new();
        store.insert(device_request("expired", -1)); // already expired
        store.insert(device_request("valid", 900));
        store.purge_expired();
        let remaining = store.list();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request_id, "valid");
    }

    #[test]
    fn insert_idempotent() {
        let store = MemoryPendingPairings::new();
        store.insert(device_request("req_1", 900));
        store.insert(device_request("req_1", 900)); // replace same id
        assert_eq!(store.list().len(), 1);
    }
}
