use std::sync::Arc;

use ozzie_core::conscience::{ApprovalRequester, ApprovalResponse};
use ozzie_core::domain::ToolError;
use ozzie_core::events::{Event, EventBus, EventKind, EventPayload, EventSource};

/// Approval requester that uses the event bus to prompt the user.
///
/// Publishes a `PromptRequest` event with the tool name and arguments,
/// then waits for a matching `PromptResponse` event. The user (via TUI/WS)
/// responds with one of: "once", "session", or "deny".
pub struct EventBusApprovalRequester {
    bus: Arc<dyn EventBus>,
}

impl EventBusApprovalRequester {
    pub fn new(bus: Arc<dyn EventBus>) -> Self {
        Self { bus }
    }
}

#[async_trait::async_trait]
impl ApprovalRequester for EventBusApprovalRequester {
    async fn request_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &str,
    ) -> Result<ApprovalResponse, ToolError> {
        // Generate a unique token for this prompt
        let token = format!("approval-{}-{}", tool_name, uuid_v4_simple());

        // Publish prompt request
        let label = ozzie_core::conscience::prompt_label(tool_name, arguments);

        // Subscribe to responses before publishing the request
        let mut rx = self.bus.subscribe(&[EventKind::PromptResponse.as_str()]);

        self.bus.publish(Event::with_session(
            EventSource::Agent,
            EventPayload::PromptRequest {
                prompt_type: "select".to_string(),
                label,
                token: token.clone(),
                options: ApprovalResponse::prompt_options(),
            },
            session_id,
        ));

        // Wait for a matching response
        loop {
            match rx.recv().await {
                Ok(event) => {
                    // Match by token
                    let (event_token, value) = match &event.payload {
                        EventPayload::PromptResponse {
                            token: t, value, ..
                        } => (t.as_str(), value.as_deref().unwrap_or("deny")),
                        _ => continue,
                    };
                    if event_token != token {
                        continue;
                    }

                    return Ok(ApprovalResponse::from_wire(value));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(ToolError::Execution(
                        "event bus closed while waiting for approval".to_string(),
                    ));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Continue listening
                    continue;
                }
            }
        }
    }
}

/// Generates a simple pseudo-unique ID (timestamp + counter).
fn uuid_v4_simple() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let seq = CTR.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{now:x}-{seq:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::events::Bus;
    use std::collections::HashMap;

    #[tokio::test]
    async fn approval_allow_once() {
        let bus = Arc::new(Bus::new(64));
        let approver = EventBusApprovalRequester::new(bus.clone());

        // Subscribe to prompt requests to extract the token
        let mut req_rx = bus.subscribe(&[EventKind::PromptRequest.as_str()]);

        let bus2 = bus.clone();
        let handle = tokio::spawn(async move {
            approver
                .request_approval("s1", "execute", "{\"command\": \"ls\"}")
                .await
        });

        // Wait for the prompt request
        let req = tokio::time::timeout(std::time::Duration::from_secs(2), req_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        let token = match &req.payload {
            EventPayload::PromptRequest { token, .. } => token.clone(),
            _ => panic!("expected PromptRequest"),
        };

        // Respond with "once"
        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token,
                value: Some("once".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("timeout")
            .expect("join")
            .unwrap();
        assert_eq!(result, ApprovalResponse::AllowOnce);
    }

    #[tokio::test]
    async fn approval_allow_session() {
        let bus = Arc::new(Bus::new(64));
        let approver = EventBusApprovalRequester::new(bus.clone());
        let mut req_rx = bus.subscribe(&[EventKind::PromptRequest.as_str()]);

        let bus2 = bus.clone();
        let handle = tokio::spawn(async move {
            approver
                .request_approval("s1", "execute", "{}")
                .await
        });

        let req = tokio::time::timeout(std::time::Duration::from_secs(2), req_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        let token = match &req.payload {
            EventPayload::PromptRequest { token, .. } => token.clone(),
            _ => panic!("expected PromptRequest"),
        };

        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token,
                value: Some("session".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("timeout")
            .expect("join")
            .unwrap();
        assert_eq!(result, ApprovalResponse::AllowSession);
    }

    #[tokio::test]
    async fn approval_deny() {
        let bus = Arc::new(Bus::new(64));
        let approver = EventBusApprovalRequester::new(bus.clone());
        let mut req_rx = bus.subscribe(&[EventKind::PromptRequest.as_str()]);

        let bus2 = bus.clone();
        let handle = tokio::spawn(async move {
            approver
                .request_approval("s1", "execute", "{}")
                .await
        });

        let req = tokio::time::timeout(std::time::Duration::from_secs(2), req_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        let token = match &req.payload {
            EventPayload::PromptRequest { token, .. } => token.clone(),
            _ => panic!("expected PromptRequest"),
        };

        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token,
                value: Some("deny".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("timeout")
            .expect("join")
            .unwrap();
        assert_eq!(result, ApprovalResponse::Deny);
    }

    #[tokio::test]
    async fn ignores_unrelated_tokens() {
        let bus = Arc::new(Bus::new(64));
        let approver = EventBusApprovalRequester::new(bus.clone());
        let mut req_rx = bus.subscribe(&[EventKind::PromptRequest.as_str()]);

        let bus2 = bus.clone();
        let handle = tokio::spawn(async move {
            approver
                .request_approval("s1", "execute", "{}")
                .await
        });

        let req = tokio::time::timeout(std::time::Duration::from_secs(2), req_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");
        let token = match &req.payload {
            EventPayload::PromptRequest { token, .. } => token.clone(),
            _ => panic!("expected PromptRequest"),
        };

        // Send response with wrong token — should be ignored
        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token: "wrong-token".to_string(),
                value: Some("once".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));

        // Now send the right token
        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token,
                value: Some("deny".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("timeout")
            .expect("join")
            .unwrap();
        assert_eq!(result, ApprovalResponse::Deny);
    }

    #[tokio::test]
    async fn prompt_includes_label_and_options() {
        let bus = Arc::new(Bus::new(64));
        let approver = EventBusApprovalRequester::new(bus.clone());
        let mut req_rx = bus.subscribe(&[EventKind::PromptRequest.as_str()]);

        let bus2 = bus.clone();
        tokio::spawn(async move {
            // Will block waiting for response, that's ok
            let _ = approver
                .request_approval("s1", "execute", "{\"cmd\": \"rm -rf /\"}")
                .await;
        });

        let req = tokio::time::timeout(std::time::Duration::from_secs(2), req_rx.recv())
            .await
            .expect("timeout")
            .expect("recv");

        // Verify prompt structure
        assert_eq!(req.event_type(), "prompt.request");
        assert_eq!(req.session_id.as_deref(), Some("s1"));

        let (label, options) = match &req.payload {
            EventPayload::PromptRequest {
                label, options, ..
            } => (label.as_str(), options),
            _ => panic!("expected PromptRequest"),
        };
        assert!(label.contains("execute"));
        assert!(label.contains("requires approval"));
        assert_eq!(options.len(), 3);

        // Respond to unblock the spawn
        let token = match &req.payload {
            EventPayload::PromptRequest { token, .. } => token.clone(),
            _ => panic!("expected PromptRequest"),
        };
        bus2.publish(Event::with_session(
            EventSource::Hub,
            EventPayload::PromptResponse {
                token,
                value: Some("deny".to_string()),
                extra: HashMap::new(),
            },
            "s1",
        ));
    }
}
