use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use tokio::sync::Mutex;

use tracing::warn;

use crate::server::AppState;

/// The result of a completed device pairing request.
pub enum DeviceApprovalResult {
    Approved { device_id: String, token: String },
    Rejected,
}

/// Short-lived in-memory cache mapping `request_id` → approval result.
///
/// The polling client consumes the `Approved` entry on first read.
/// `Rejected` entries are kept briefly so the client can learn the outcome.
pub struct DeviceApprovalCache {
    entries: Mutex<HashMap<String, DeviceApprovalResult>>,
}

impl DeviceApprovalCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub async fn store_approved(&self, request_id: &str, device_id: &str, token: &str) {
        self.entries.lock().await.insert(
            request_id.to_string(),
            DeviceApprovalResult::Approved {
                device_id: device_id.to_string(),
                token: token.to_string(),
            },
        );
    }

    pub async fn store_rejected(&self, request_id: &str) {
        self.entries
            .lock()
            .await
            .insert(request_id.to_string(), DeviceApprovalResult::Rejected);
    }

    /// Returns the approval result and removes `Approved` entries (one-time claim).
    pub async fn take(&self, request_id: &str) -> Option<DeviceApprovalResult> {
        let mut map = self.entries.lock().await;
        match map.get(request_id) {
            Some(DeviceApprovalResult::Approved { .. }) => map.remove(request_id),
            Some(DeviceApprovalResult::Rejected) => {
                // Leave rejected entries so repeated polls can see the result.
                Some(DeviceApprovalResult::Rejected)
            }
            None => None,
        }
    }
}

impl Default for DeviceApprovalCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Request body for `POST /api/pair`.
#[derive(serde::Deserialize)]
pub struct CreatePairBody {
    pub client_type: String,
    pub label: Option<String>,
    /// The client's device key (`$OZZIE_PATH/.key`).
    /// When it matches the gateway's own key, the request is auto-approved.
    pub device_key: Option<String>,
}

/// POST /api/pair — submits a device pairing request.
///
/// Returns `{ request_id, expires_at }`. The client polls `GET /api/pair/{id}`
/// until the admin approves or rejects via `ozzie pairing requests approve`.
pub async fn create_pair_request(
    State(state): State<AppState>,
    Json(body): Json<CreatePairBody>,
) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "pairing not configured"})),
        )
            .into_response();
    };

    let request_id = pm.create_device_request(&body.client_type, body.label.as_deref());

    // Auto-approve when the client's device_key matches the gateway's own key
    // (i.e. they share the same OZZIE_PATH — the "same-home" shortcut).
    let same_home = matches!(
        (&body.device_key, &state.local_key),
        (Some(client_key), Some(gateway_key)) if client_key == gateway_key
    );
    if same_home {
        let device_id = uuid::Uuid::new_v4().to_string();
        let token = uuid::Uuid::new_v4().to_string();
        if let Some(ref ds) = state.device_storage
            && let Err(e) = pm.approve_device(&request_id, &device_id, &token, ds.as_ref())
        {
            warn!(request_id = %request_id, error = %e, "failed to auto-approve device pairing");
        }
        if let Some(ref approvals) = state.device_approvals {
            approvals.store_approved(&request_id, &device_id, &token).await;
        }
    }

    // Compute approximate expiry (matches REQUEST_TTL_MINUTES = 15).
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(15);

    Json(serde_json::json!({
        "request_id": request_id,
        "expires_at": expires_at.to_rfc3339(),
    }))
    .into_response()
}

/// GET /api/pair/:id — polls for the status of a device pairing request.
///
/// Returns `{ status: "pending" | "approved" | "rejected", device_id?, token? }`.
pub async fn poll_pair_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "pairing not configured"})),
        )
            .into_response();
    };

    // Check the approval cache first.
    if let Some(ref approvals) = state.device_approvals {
        match approvals.take(&request_id).await {
            Some(DeviceApprovalResult::Approved { device_id, token }) => {
                return Json(serde_json::json!({
                    "status": "approved",
                    "device_id": device_id,
                    "token": token,
                }))
                .into_response();
            }
            Some(DeviceApprovalResult::Rejected) => {
                return Json(serde_json::json!({"status": "rejected"})).into_response();
            }
            None => {}
        }
    }

    // Check whether still pending.
    let still_pending = pm
        .list_pending()
        .iter()
        .any(|r| r.request_id() == request_id);

    if still_pending {
        Json(serde_json::json!({"status": "pending"})).into_response()
    } else {
        // Unknown — either expired or already claimed.
        Json(serde_json::json!({"status": "unknown"})).into_response()
    }
}
