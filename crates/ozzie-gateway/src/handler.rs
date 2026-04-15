use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;

use ozzie_core::conscience::ToolPermissions;
use ozzie_core::events::{Event, EventBus, EventPayload, EventSource};
use ozzie_protocol::{error_code, Frame, Request};
use ozzie_runtime::session::{Session, SessionStore};
use ozzie_types::{
    AcceptedResult, CancelledResult, MessagePayload, MessagesResult, SessionResult,
};

use crate::hub::{Hub, HubHandler};

/// Callback to cancel a session's active ReactLoop.
pub type CancelSessionFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Default handler for WS request frames.
pub struct RequestHandler {
    bus: Arc<dyn EventBus>,
    sessions: Arc<dyn SessionStore>,
    hub: Arc<Hub>,
    permissions: Option<Arc<ToolPermissions>>,
    cancel_fn: Option<CancelSessionFn>,
    blob_store: Option<Arc<dyn ozzie_core::domain::BlobStore>>,
}

impl RequestHandler {
    pub fn new(
        bus: Arc<dyn EventBus>,
        sessions: Arc<dyn SessionStore>,
        hub: Arc<Hub>,
    ) -> Self {
        Self {
            bus,
            sessions,
            hub,
            permissions: None,
            cancel_fn: None,
            blob_store: None,
        }
    }

    pub fn with_blob_store(mut self, store: Arc<dyn ozzie_core::domain::BlobStore>) -> Self {
        self.blob_store = Some(store);
        self
    }

    pub fn with_permissions(mut self, permissions: Arc<ToolPermissions>) -> Self {
        self.permissions = Some(permissions);
        self
    }

    pub fn with_cancel_fn(mut self, cancel_fn: CancelSessionFn) -> Self {
        self.cancel_fn = Some(cancel_fn);
        self
    }
}

#[async_trait::async_trait]
impl HubHandler for RequestHandler {
    async fn handle_request(&self, client_id: u64, frame: Frame) -> Frame {
        let id = frame.id.clone().unwrap_or_default();

        let request = match frame.parse_request() {
            Ok(r) => r,
            Err(e) => {
                let method = frame.method.as_deref().unwrap_or("?");
                return Frame::response_err(
                    &id,
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown or invalid method '{method}': {e}"),
                );
            }
        };

        match request {
            Request::OpenSession(p) => self.handle_open_session(client_id, &id, p).await,
            Request::SendMessage(p) => self.handle_send_message(&id, p).await,
            Request::SendConnectorMessage(p) => self.handle_send_connector_message(&id, p),
            Request::LoadMessages(p) => self.handle_load_messages(&id, p).await,
            Request::AcceptAllTools(p) => self.handle_accept_all(&id, p),
            Request::PromptResponse(p) => self.handle_prompt_response(&id, p),
            Request::CancelSession(p) => self.handle_cancel_session(&id, p),
        }
    }
}

impl RequestHandler {
    async fn handle_open_session(
        &self,
        client_id: u64,
        req_id: &str,
        p: ozzie_types::OpenSessionParams,
    ) -> Frame {
        let session = if let Some(sid) = &p.session_id {
            match self.sessions.get(sid).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return Frame::response_err(
                        req_id,
                        error_code::INVALID_PARAMS,
                        format!("session not found: {sid}"),
                    );
                }
                Err(e) => {
                    return Frame::response_err(
                        req_id,
                        error_code::INTERNAL_ERROR,
                        format!("store error: {e}"),
                    );
                }
            }
        } else {
            let mut session = Session::new(
                ozzie_utils::names::generate_id("sess", &|_: &str| false),
            );
            session.root_dir = p.working_dir;
            session.language = p.language;
            session.model = p.model;

            if let Err(e) = self.sessions.create(&session).await {
                return Frame::response_err(
                    req_id,
                    error_code::INTERNAL_ERROR,
                    format!("create session: {e}"),
                );
            }

            self.bus.publish(Event::with_session(
                EventSource::Hub,
                EventPayload::SessionCreated {
                    session_id: session.id.clone(),
                },
                &session.id,
            ));

            session
        };

        self.hub.bind_session(client_id, &session.id);
        info!(session_id = %session.id, "session opened");

        Frame::response_ok(
            req_id,
            &SessionResult {
                session_id: session.id,
                root_dir: session.root_dir,
            },
        )
    }

    async fn handle_send_message(
        &self,
        req_id: &str,
        p: ozzie_types::SendMessageParams,
    ) -> Frame {
        // Ingest image attachments into blob store, collect refs
        let images = if p.images.is_empty() {
            Vec::new()
        } else {
            let mut refs = Vec::new();
            for img in &p.images {
                match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &img.data) {
                    Ok(bytes) => {
                        if let Some(ref store) = self.blob_store {
                            match store.write(&bytes, &img.media_type).await {
                                Ok(blob_ref) => refs.push(blob_ref),
                                Err(e) => tracing::warn!(error = %e, "failed to write blob"),
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "invalid base64 image data"),
                }
            }
            refs
        };

        self.bus.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::user_message_with_images(p.text, images),
            &p.session_id,
        ));

        Frame::response_ok(req_id, &AcceptedResult { accepted: true })
    }

    fn handle_send_connector_message(
        &self,
        req_id: &str,
        p: ozzie_types::SendConnectorMessageParams,
    ) -> Frame {
        let identity = serde_json::json!({
            "platform": p.connector,
            "user_id": p.author,
            "name": p.author,
            "channel_id": p.channel_id,
            "server_id": p.server_id.clone().unwrap_or_default(),
        });

        self.bus.publish(Event::new(
            EventSource::Connector,
            EventPayload::ConnectorMessage {
                connector: p.connector,
                channel_id: p.channel_id,
                message_id: p.message_id.unwrap_or_default(),
                content: p.content,
                identity: Some(identity),
                roles: Vec::new(),
            },
        ));

        Frame::response_ok(req_id, &AcceptedResult { accepted: true })
    }

    async fn handle_load_messages(
        &self,
        req_id: &str,
        p: ozzie_types::LoadMessagesParams,
    ) -> Frame {
        let limit = p.limit.min(50) as usize;

        let messages = match self.sessions.load_messages(&p.session_id).await {
            Ok(msgs) => msgs,
            Err(e) => {
                return Frame::response_err(
                    req_id,
                    error_code::INTERNAL_ERROR,
                    format!("load messages: {e}"),
                );
            }
        };

        let items: Vec<MessagePayload> = messages
            .iter()
            .rev()
            .take(limit)
            .rev()
            .map(|m| MessagePayload {
                role: m.role.clone(),
                content: m.content.clone(),
                user_visible: m.user_visible,
                agent_visible: m.agent_visible,
            })
            .collect();

        Frame::response_ok(req_id, &MessagesResult { messages: items })
    }

    fn handle_accept_all(
        &self,
        req_id: &str,
        p: ozzie_types::AcceptAllToolsParams,
    ) -> Frame {
        if let Some(ref perms) = self.permissions {
            perms.allow_all_for_session(&p.session_id);
        }

        Frame::response_ok(req_id, &AcceptedResult { accepted: true })
    }

    fn handle_prompt_response(
        &self,
        req_id: &str,
        p: ozzie_types::PromptResponseParams,
    ) -> Frame {
        self.bus.publish(Event::new(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token: p.token,
                value: p.value,
                extra: HashMap::new(),
            },
        ));

        Frame::response_ok(req_id, &AcceptedResult { accepted: true })
    }

    fn handle_cancel_session(
        &self,
        req_id: &str,
        p: ozzie_types::CancelSessionParams,
    ) -> Frame {
        if let Some(ref cancel) = self.cancel_fn {
            cancel(&p.session_id);
        }

        self.bus.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::AgentCancelled {
                reason: "user_request".to_string(),
            },
            &p.session_id,
        ));

        Frame::response_ok(req_id, &CancelledResult { cancelled: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::events::Bus;
    use ozzie_runtime::session::InMemorySessionStore;

    struct NoopHandler;

    #[async_trait::async_trait]
    impl HubHandler for NoopHandler {
        async fn handle_request(&self, _client_id: u64, frame: Frame) -> Frame {
            Frame::response_ok(frame.id.unwrap_or_default(), &serde_json::json!({}))
        }
    }

    fn make_handler() -> (RequestHandler, Arc<Bus>) {
        let bus = Arc::new(Bus::new(64));
        let sessions = Arc::new(InMemorySessionStore::new());
        let hub = Hub::new(bus.clone(), Arc::new(NoopHandler) as Arc<dyn HubHandler>);
        let handler = RequestHandler::new(
            bus.clone() as Arc<dyn EventBus>,
            sessions as Arc<dyn SessionStore>,
            hub,
        );
        (handler, bus)
    }

    #[tokio::test]
    async fn send_message_publishes_user_message() {
        let (handler, bus) = make_handler();
        let mut rx = bus.subscribe(&["user.message"]);

        let frame = Frame::request(
            "r1",
            "send_message",
            &serde_json::json!({"session_id": "sess_1", "text": "hello"}),
        );
        let resp = handler.handle_request(0, frame).await;

        assert!(resp.is_response());
        assert!(!resp.is_error());

        let event = rx.try_recv().unwrap();
        match event.payload {
            EventPayload::UserMessage { text, .. } => assert_eq!(text, "hello"),
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_connector_message_publishes_connector_message() {
        let (handler, bus) = make_handler();
        let mut rx = bus.subscribe(&["connector.message"]);

        let frame = Frame::request(
            "r1",
            "send_connector_message",
            &serde_json::json!({
                "connector": "discord",
                "channel_id": "ch_123",
                "author": "alice",
                "content": "hello from discord",
                "message_id": "msg_456",
            }),
        );
        let resp = handler.handle_request(0, frame).await;

        assert!(resp.is_response());
        assert!(!resp.is_error());

        let event = rx.try_recv().unwrap();
        match event.payload {
            EventPayload::ConnectorMessage {
                connector,
                channel_id,
                message_id,
                content,
                identity,
                roles,
            } => {
                assert_eq!(connector, "discord");
                assert_eq!(channel_id, "ch_123");
                assert_eq!(message_id, "msg_456");
                assert_eq!(content, "hello from discord");
                assert!(identity.is_some());
                let id = identity.unwrap();
                assert_eq!(id["platform"], "discord");
                assert_eq!(id["user_id"], "alice");
                assert_eq!(id["channel_id"], "ch_123");
                assert!(roles.is_empty());
            }
            other => panic!("expected ConnectorMessage, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_connector_message_without_message_id() {
        let (handler, bus) = make_handler();
        let mut rx = bus.subscribe(&["connector.message"]);

        let frame = Frame::request(
            "r2",
            "send_connector_message",
            &serde_json::json!({
                "connector": "file",
                "channel_id": "bench",
                "author": "user",
                "content": "test",
            }),
        );
        let resp = handler.handle_request(0, frame).await;
        assert!(!resp.is_error());

        let event = rx.try_recv().unwrap();
        match event.payload {
            EventPayload::ConnectorMessage { message_id, .. } => {
                assert_eq!(message_id, "");
            }
            other => panic!("expected ConnectorMessage, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let (handler, _bus) = make_handler();

        let frame = Frame::request("r1", "nonexistent_method", &serde_json::json!({}));
        let resp = handler.handle_request(0, frame).await;

        assert!(resp.is_error());
    }

    #[tokio::test]
    async fn open_session_creates_session() {
        let (handler, _bus) = make_handler();

        let frame = Frame::request("r1", "open_session", &serde_json::json!({}));
        let resp = handler.handle_request(0, frame).await;

        assert!(!resp.is_error());
        let sid = resp
            .result
            .as_ref()
            .and_then(|r| r.get("session_id"))
            .and_then(|v| v.as_str());
        assert!(sid.is_some());
        assert!(sid.unwrap().starts_with("sess_"));
    }
}
