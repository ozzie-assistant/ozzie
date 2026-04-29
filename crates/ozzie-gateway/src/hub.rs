use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{info, warn};

use ozzie_core::events::EventBus;

use crate::protocol::Frame;

/// Unique client identifier.
type ClientId = u64;

/// A connected WebSocket client.
struct Client {
    conversation_id: Option<String>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

/// WebSocket hub managing connected clients and event bridging.
pub struct Hub {
    clients: RwLock<HashMap<ClientId, Client>>,
    next_id: Mutex<ClientId>,
    bus: Arc<dyn EventBus>,
    handler: RwLock<Arc<dyn HubHandler>>,
}

/// Handles incoming WS requests from clients.
#[async_trait::async_trait]
pub trait HubHandler: Send + Sync {
    /// Handles a request frame from a client and returns a response frame.
    async fn handle_request(&self, client_id: u64, frame: Frame) -> Frame;
}

impl Hub {
    /// Creates a new hub.
    pub fn new(bus: Arc<dyn EventBus>, handler: Arc<dyn HubHandler>) -> Arc<Self> {
        let hub = Arc::new(Self {
            clients: RwLock::new(HashMap::new()),
            next_id: Mutex::new(1),
            bus,
            handler: RwLock::new(handler),
        });

        // Start event bridging
        let hub_clone = hub.clone();
        tokio::spawn(async move {
            hub_clone.bridge_events().await;
        });

        hub
    }

    /// Replaces the handler used to process incoming requests.
    pub fn set_handler(&self, handler: Arc<dyn HubHandler>) {
        *self.handler.write().unwrap_or_else(|e| e.into_inner()) = handler;
    }

    /// Registers a new client and runs its read/write loops.
    pub async fn handle_socket(self: &Arc<Self>, socket: WebSocket) {
        let (mut ws_tx, mut ws_rx) = socket.split();

        let client_id = {
            let mut next = self.next_id.lock().unwrap_or_else(|e| e.into_inner());
            let id = *next;
            *next += 1;
            id
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Register client
        {
            let mut clients = self.clients.write().unwrap_or_else(|e| e.into_inner());
            clients.insert(
                client_id,
                Client {
                    conversation_id: None,
                    tx,
                },
            );
        }

        info!(client_id, "client connected");

        // Write pump: forward messages from channel to WebSocket
        let write_handle = tokio::spawn(async move {
            while let Some(data) = rx.recv().await {
                let text = String::from_utf8_lossy(&data).into_owned();
                if ws_tx
                    .send(WsMessage::Text(text.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Read pump: receive frames from WebSocket, dispatch to handler
        let hub = self.clone();
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                WsMessage::Text(text) => {
                    hub.handle_message(client_id, text.as_bytes()).await;
                }
                WsMessage::Binary(data) => {
                    hub.handle_message(client_id, &data).await;
                }
                WsMessage::Close(_) => break,
                _ => {}
            }
        }

        // Unregister
        let conversation_id = {
            let mut clients = self.clients.write().unwrap_or_else(|e| e.into_inner());
            let session = clients.get(&client_id).and_then(|c| c.conversation_id.clone());
            clients.remove(&client_id);
            session
        };

        write_handle.abort();

        info!(client_id, ?conversation_id, "client disconnected");

        // Check if last client in session
        // Conversations are no longer tied to client connections; nothing
        // to emit when the last WebSocket client of a conversation departs.
        let _ = conversation_id;
    }

    async fn handle_message(&self, client_id: ClientId, data: &[u8]) {
        let frame = match Frame::from_bytes(data) {
            Ok(f) => f,
            Err(e) => {
                warn!(client_id, error = %e, "invalid frame");
                return;
            }
        };

        if !frame.is_request() {
            return;
        }

        let handler = self.handler.read().unwrap_or_else(|e| e.into_inner()).clone();
        let response = handler.handle_request(client_id, frame).await;
        let bytes = match response.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                warn!(client_id, error = %e, "failed to serialize response");
                return;
            }
        };

        self.send_to_client(client_id, &bytes);
    }

    /// Associates a client with a session.
    pub fn bind_session(&self, client_id: ClientId, conversation_id: &str) {
        let mut clients = self.clients.write().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = clients.get_mut(&client_id) {
            client.conversation_id = Some(conversation_id.to_string());
        }
    }

    /// Broadcasts data to all clients in a session.
    pub fn send_to_session(&self, conversation_id: &str, data: &[u8]) {
        let clients = self.clients.read().unwrap_or_else(|e| e.into_inner());
        for client in clients.values() {
            if client.conversation_id.as_deref() == Some(conversation_id) {
                let _ = client.tx.send(data.to_vec());
            }
        }
    }

    /// Broadcasts data to all connected clients.
    pub fn broadcast(&self, data: &[u8]) {
        let clients = self.clients.read().unwrap_or_else(|e| e.into_inner());
        for client in clients.values() {
            let _ = client.tx.send(data.to_vec());
        }
    }

    /// Returns the number of connected clients.
    pub fn client_count(&self) -> usize {
        self.clients.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn send_to_client(&self, client_id: ClientId, data: &[u8]) {
        let clients = self.clients.read().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = clients.get(&client_id) {
            let _ = client.tx.send(data.to_vec());
        }
    }

    /// Bridges events from the event bus to connected clients.
    async fn bridge_events(&self) {
        let mut rx = self.bus.subscribe(&[]);

        loop {
            match rx.recv().await {
                Ok(event) => {
                    let event_type_str = event.event_type();
                    let payload_value =
                        serde_json::to_value(&event.payload).unwrap_or_default();
                    let frame = Frame::event(
                        event_type_str,
                        event.conversation_id.as_deref(),
                        &payload_value,
                    );

                    let Ok(bytes) = frame.to_bytes() else {
                        continue;
                    };

                    if let Some(ref conversation_id) = event.conversation_id {
                        self.send_to_session(conversation_id, &bytes);
                    } else {
                        self.broadcast(&bytes);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "event bridge lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("event bus closed, stopping bridge");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::events::Bus;

    struct EchoHandler;

    #[async_trait::async_trait]
    impl HubHandler for EchoHandler {
        async fn handle_request(&self, _client_id: u64, frame: Frame) -> Frame {
            Frame::response_ok(
                frame.id.unwrap_or_default(),
                &serde_json::json!({"echo": true}),
            )
        }
    }

    #[test]
    fn hub_creation() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bus = Arc::new(Bus::new(64));
            let handler = Arc::new(EchoHandler);
            let hub = Hub::new(bus, handler);
            assert_eq!(hub.client_count(), 0);
        });
    }

    #[test]
    fn send_to_session_no_clients() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bus = Arc::new(Bus::new(64));
            let handler = Arc::new(EchoHandler);
            let hub = Hub::new(bus, handler);
            // Should not panic with no clients
            hub.send_to_session("sess_nonexistent", b"test");
        });
    }

    #[test]
    fn broadcast_no_clients() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let bus = Arc::new(Bus::new(64));
            let handler = Arc::new(EchoHandler);
            let hub = Hub::new(bus, handler);
            hub.broadcast(b"test");
        });
    }
}
