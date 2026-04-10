use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use ozzie_core::auth::Authenticator;
use ozzie_core::events::EventBus;

use crate::auth::auth_middleware;
use crate::hub::Hub;
use crate::memory_api;
use crate::pair_device::{self, DeviceApprovalCache};
use crate::pairing;
use crate::profile_api;

/// Shared state for axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Hub>,
    pub bus: Arc<dyn EventBus>,
    /// Optional authenticator. If `None`, all requests pass (insecure mode).
    pub authenticator: Option<Arc<dyn Authenticator>>,
    /// Session store for REST API.
    pub sessions: Option<Arc<dyn ozzie_runtime::SessionStore>>,
    /// Pairing manager for chat connector pairing flows.
    pub pairing_manager: Option<Arc<ozzie_runtime::PairingManager>>,
    /// Chat pairing storage for direct disk operations.
    pub chat_storage: Option<Arc<dyn ozzie_core::domain::PairingStorage>>,
    /// Device storage for verifying device tokens.
    pub device_storage: Option<Arc<dyn ozzie_core::domain::DeviceStorage>>,
    /// Cache for approved/rejected device pairing results.
    pub device_approvals: Option<Arc<DeviceApprovalCache>>,
    /// The gateway's own device key (`$OZZIE_PATH/.key`).
    /// Pairing requests that carry this key are auto-approved (same-home shortcut).
    pub local_key: Option<String>,
    /// Memory store for REST API (entries).
    pub memory_store: Option<Arc<dyn ozzie_core::domain::MemoryStore>>,
    /// Page store for REST API (wiki pages).
    pub page_store: Option<Arc<dyn ozzie_core::domain::PageStore>>,
    /// Ozzie data directory (for loading profile, schema, etc.).
    pub ozzie_path: PathBuf,
}

/// Gateway server configuration.
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 18420,
        }
    }
}

/// HTTP/WebSocket gateway server.
pub struct Server {
    config: ServerConfig,
    state: AppState,
}

impl Server {
    pub fn new(config: ServerConfig, state: AppState) -> Self {
        Self { config, state }
    }

    /// Builds the axum router.
    ///
    /// Auth middleware is applied to all routes except `/api/health`.
    /// If no authenticator is configured, all requests pass (insecure mode).
    pub fn router(&self) -> Router {
        let auth = self.state.authenticator.clone();

        // Protected routes — auth middleware applied.
        let protected = Router::new()
            .route("/api/ws", get(ws_upgrade))
            .route("/api/events", get(events))
            .route("/api/pairings/requests", get(pairing::list_pairing_requests))
            .route(
                "/api/pairings/requests/{id}/approve",
                post(pairing::approve_pairing_request),
            )
            .route(
                "/api/pairings/requests/{id}/reject",
                post(pairing::reject_pairing_request),
            )
            .route("/api/pairings/chats", get(pairing::list_chat_pairings))
            .route(
                "/api/pairings/chats",
                delete(pairing::remove_chat_pairing),
            )
            // Memory/Wiki API
            .route("/api/memory/entries", get(memory_api::list_entries))
            .route("/api/memory/entries/search", get(memory_api::search_entries))
            .route("/api/memory/entries/{id}", get(memory_api::get_entry))
            .route("/api/memory/pages", get(memory_api::list_pages))
            .route("/api/memory/pages/search", get(memory_api::search_pages))
            .route("/api/memory/pages/{slug}", get(memory_api::get_page))
            .route("/api/memory/index", get(memory_api::get_index))
            .route("/api/memory/schema", get(memory_api::get_schema))
            // Profile API
            .route("/api/profile", get(profile_api::get_profile))
            .route("/api/profile", axum::routing::put(profile_api::update_profile))
            .route("/api/profile/whoami", get(profile_api::get_whoami))
            .layer(middleware::from_fn(move |req, next| {
                let auth = auth.clone();
                auth_middleware(auth, req, next)
            }));

        // Public routes — no auth.
        Router::new()
            .route("/api/health", get(health))
            .route("/api/sessions", get(list_sessions))
            .route("/api/pair", post(pair_device::create_pair_request))
            .route("/api/pair/{id}", get(pair_device::poll_pair_request))
            .merge(protected)
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive())
            .with_state(self.state.clone())
    }

    /// Starts the server and blocks until shutdown.
    pub async fn serve(self) -> Result<(), GatewayError> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|e| GatewayError::Bind(format!("invalid address: {e}")))?;

        let router = self.router();

        info!(addr = %addr, "gateway listening");

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::Bind(e.to_string()))?;

        axum::serve(listener, router)
            .await
            .map_err(|e| GatewayError::Serve(e.to_string()))
    }
}

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({"status": "ok"}))
}

/// Lists all sessions with metadata.
async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let Some(store) = &state.sessions else {
        return axum::Json(serde_json::json!([]));
    };

    match store.list().await {
        Ok(sessions) => {
            let items: Vec<serde_json::Value> = sessions
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "status": format!("{:?}", s.status),
                        "created_at": s.created_at.to_rfc3339(),
                        "updated_at": s.updated_at.to_rfc3339(),
                        "title": s.title,
                        "language": s.language,
                        "model": s.model,
                        "message_count": s.message_count,
                        "token_usage": {
                            "input": s.token_usage.input,
                            "output": s.token_usage.output,
                        },
                    })
                })
                .collect();
            axum::Json(serde_json::Value::Array(items))
        }
        Err(_) => axum::Json(serde_json::json!([])),
    }
}

/// WebSocket upgrade endpoint.
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket: WebSocket| async move {
        state.hub.handle_socket(socket).await;
    })
}

/// Event query parameters.
#[derive(serde::Deserialize)]
struct EventQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(rename = "type")]
    event_type: Option<String>,
    session: Option<String>,
}

fn default_limit() -> usize {
    50
}

/// Events history endpoint.
async fn events(
    Query(query): Query<EventQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let limit = query.limit.min(500);

    let events = if let Some(ref type_str) = query.event_type {
        state.bus.history_filtered(limit, type_str)
    } else {
        state.bus.history(limit)
    };

    // Optional session filter (client-side)
    let filtered: Vec<_> = if let Some(ref session_id) = query.session {
        events
            .into_iter()
            .filter(|e| e.session_id.as_deref() == Some(session_id.as_str()))
            .collect()
    } else {
        events
    };

    let items: Vec<serde_json::Value> = filtered
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "type": e.event_type(),
                "source": e.source,
                "session_id": e.session_id,
                "timestamp": e.timestamp,
                "payload": e.payload,
            })
        })
        .collect();

    axum::Json(serde_json::json!({"events": items}))
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("bind failed: {0}")]
    Bind(String),
    #[error("serve failed: {0}")]
    Serve(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use ozzie_core::events::{Bus, EventPayload, EventSource};
    use tower::ServiceExt;

    use crate::hub::HubHandler;
    use crate::protocol::Frame;

    struct NoopHandler;

    #[async_trait::async_trait]
    impl HubHandler for NoopHandler {
        async fn handle_request(&self, _client_id: u64, frame: Frame) -> Frame {
            Frame::response_ok(frame.id.unwrap_or_default(), &serde_json::json!({}))
        }
    }

    fn make_state() -> AppState {
        let bus = Arc::new(Bus::new(64));
        let handler = Arc::new(NoopHandler);
        let hub = Hub::new(bus.clone(), handler);
        AppState {
            hub,
            bus: bus as Arc<dyn EventBus>,
            authenticator: None,
            sessions: None,
            pairing_manager: None,
            chat_storage: None,
            device_storage: None,
            device_approvals: None,
            local_key: None,
            memory_store: None,
            page_store: None,
            ozzie_path: std::path::PathBuf::new(),
        }
    }

    #[tokio::test]
    async fn health_endpoint() {
        let state = make_state();
        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn events_endpoint_empty() {
        let state = make_state();
        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    fn make_state_with_auth(auth: Option<Arc<dyn ozzie_core::auth::Authenticator>>) -> AppState {
        let bus = Arc::new(Bus::new(64));
        let handler = Arc::new(NoopHandler);
        let hub = Hub::new(bus.clone(), handler);
        AppState {
            hub,
            bus: bus as Arc<dyn EventBus>,
            authenticator: auth,
            sessions: None,
            pairing_manager: None,
            chat_storage: None,
            device_storage: None,
            device_approvals: None,
            local_key: None,
            memory_store: None,
            page_store: None,
            ozzie_path: std::path::PathBuf::new(),
        }
    }

    #[tokio::test]
    async fn health_always_public() {
        // Even with auth configured, /api/health should be accessible.
        let auth = Arc::new(ozzie_core::auth::LocalAuth::new("secret"));
        let state = make_state_with_auth(Some(auth));
        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn events_requires_auth() {
        let auth = Arc::new(ozzie_core::auth::LocalAuth::new("secret"));
        let state = make_state_with_auth(Some(auth));
        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        // No token → 401
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 401);
    }

    #[tokio::test]
    async fn events_with_valid_auth() {
        let auth = Arc::new(ozzie_core::auth::LocalAuth::new("secret"));
        let state = make_state_with_auth(Some(auth));
        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .header("Authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn events_with_type_filter() {
        let state = make_state();

        // Publish some events
        state.bus.publish(ozzie_core::events::Event::new(
            EventSource::Agent,
            EventPayload::user_message("hello"),
        ));

        let server = Server::new(ServerConfig::default(), state);
        let app = server.router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/events?type=user.message")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }
}
