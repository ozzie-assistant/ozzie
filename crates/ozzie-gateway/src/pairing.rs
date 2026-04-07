use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use ozzie_core::policy::PairingKey;

use crate::server::AppState;

/// Response body for approve.
#[derive(serde::Deserialize)]
pub struct ApproveBody {
    pub policy: String,
}

/// Request body for remove_chat.
#[derive(serde::Deserialize)]
pub struct ChatPairingKeyBody {
    pub platform: String,
    pub server_id: String,
    pub user_id: String,
}

fn pairing_not_configured() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error": "pairing not configured"})),
    )
}

/// GET /api/pairings/requests — lists all pending pairing requests.
pub async fn list_pairing_requests(State(state): State<AppState>) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return pairing_not_configured().into_response();
    };

    let requests = pm.list_pending();
    Json(serde_json::json!({"requests": requests})).into_response()
}

/// POST /api/pairings/requests/:id/approve — approves a pending pairing request.
///
/// For chat requests, `policy` is required. For device requests, a token is
/// generated and stored in `device_storage`; the polling client receives it
/// via `GET /api/pair/:id`.
pub async fn approve_pairing_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(body): Json<ApproveBody>,
) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return pairing_not_configured().into_response();
    };

    // Determine the kind before consuming the request.
    let is_device = pm
        .list_pending()
        .iter()
        .any(|r| r.request_id() == request_id && r.is_device());

    if is_device {
        let Some(ref ds) = state.device_storage else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "device storage not configured"})),
            )
                .into_response();
        };

        let device_id = uuid::Uuid::new_v4().to_string();
        let token = uuid::Uuid::new_v4().to_string();

        match pm.approve_device(&request_id, &device_id, &token, ds.as_ref()) {
            Ok(()) => {
                if let Some(ref approvals) = state.device_approvals {
                    approvals.store_approved(&request_id, &device_id, &token).await;
                }
                Json(serde_json::json!({"ok": true, "device_id": device_id})).into_response()
            }
            Err(ozzie_core::domain::PairingError::NotFound(msg)) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        }
    } else {
        // Default: treat as chat pairing.
        match pm.approve_chat(&request_id, &body.policy, "cli") {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
            Err(ozzie_core::domain::PairingError::NotFound(msg)) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        }
    }
}

/// POST /api/pairings/requests/:id/reject — rejects a pending pairing request.
pub async fn reject_pairing_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return pairing_not_configured().into_response();
    };

    // Check kind before consuming (for device rejection tracking).
    let is_device = pm
        .list_pending()
        .iter()
        .any(|r| r.request_id() == request_id && r.is_device());

    match pm.reject(&request_id, "cli") {
        Ok(()) => {
            if is_device
                && let Some(ref approvals) = state.device_approvals
            {
                approvals.store_rejected(&request_id).await;
            }
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Err(ozzie_core::domain::PairingError::NotFound(msg)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/pairings/chats — lists all approved chat pairings.
pub async fn list_chat_pairings(State(state): State<AppState>) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return pairing_not_configured().into_response();
    };

    let pairings = pm.list_chat_pairings();
    Json(serde_json::json!({"pairings": pairings})).into_response()
}

/// DELETE /api/pairings/chats — removes a specific chat pairing.
pub async fn remove_chat_pairing(
    State(state): State<AppState>,
    Json(body): Json<ChatPairingKeyBody>,
) -> impl IntoResponse {
    let Some(ref pm) = state.pairing_manager else {
        return pairing_not_configured().into_response();
    };

    let key = PairingKey {
        platform: body.platform,
        server_id: body.server_id,
        user_id: body.user_id,
    };

    match pm.remove_chat_pairing(&key) {
        Ok(true) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "pairing not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
