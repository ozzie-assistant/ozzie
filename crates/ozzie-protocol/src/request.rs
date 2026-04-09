use serde::{Deserialize, Serialize};

use ozzie_types::{
    AcceptAllToolsParams, CancelSessionParams, LoadMessagesParams, OpenSessionParams,
    PromptResponseParams, SendConnectorMessageParams, SendMessageParams,
};

/// Typed RPC request — each variant carries its own params.
///
/// Deserialized from `{ "method": "...", "params": { ... } }` via
/// `#[serde(tag = "method", content = "params")]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    OpenSession(OpenSessionParams),
    SendMessage(SendMessageParams),
    SendConnectorMessage(SendConnectorMessageParams),
    LoadMessages(LoadMessagesParams),
    AcceptAllTools(AcceptAllToolsParams),
    PromptResponse(PromptResponseParams),
    CancelSession(CancelSessionParams),
}

impl Request {
    /// Returns the wire method name for this request variant.
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::OpenSession(_) => "open_session",
            Self::SendMessage(_) => "send_message",
            Self::SendConnectorMessage(_) => "send_connector_message",
            Self::LoadMessages(_) => "load_messages",
            Self::AcceptAllTools(_) => "accept_all_tools",
            Self::PromptResponse(_) => "prompt_response",
            Self::CancelSession(_) => "cancel_session",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = Request::SendMessage(SendMessageParams {
            session_id: "sess_1".to_string(),
            text: "hello".to_string(),
            images: Vec::new(),
        });

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["method"], "send_message");
        assert_eq!(json["params"]["session_id"], "sess_1");
        assert_eq!(json["params"]["text"], "hello");

        let parsed: Request = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.method_name(), "send_message");
    }

    #[test]
    fn open_session_with_defaults() {
        let json = serde_json::json!({
            "method": "open_session",
            "params": {}
        });
        let req: Request = serde_json::from_value(json).unwrap();
        match req {
            Request::OpenSession(p) => {
                assert!(p.session_id.is_none());
                assert!(p.working_dir.is_none());
            }
            _ => panic!("expected OpenSession"),
        }
    }

    #[test]
    fn cancel_session_parse() {
        let json = serde_json::json!({
            "method": "cancel_session",
            "params": { "session_id": "sess_xyz" }
        });
        let req: Request = serde_json::from_value(json).unwrap();
        match req {
            Request::CancelSession(p) => {
                assert_eq!(p.session_id, "sess_xyz");
            }
            _ => panic!("expected CancelSession"),
        }
    }

    #[test]
    fn send_connector_message_roundtrip() {
        let json = serde_json::json!({
            "method": "send_connector_message",
            "params": {
                "connector": "discord",
                "channel_id": "ch_123",
                "author": "alice",
                "content": "hello from discord"
            }
        });
        let req: Request = serde_json::from_value(json).unwrap();
        match req {
            Request::SendConnectorMessage(p) => {
                assert_eq!(p.connector, "discord");
                assert_eq!(p.channel_id, "ch_123");
                assert_eq!(p.author, "alice");
                assert_eq!(p.content, "hello from discord");
                assert!(p.message_id.is_none());
            }
            _ => panic!("expected SendConnectorMessage"),
        }
    }

    #[test]
    fn method_name_matches_wire() {
        let variants = vec![
            (Request::OpenSession(Default::default()), "open_session"),
            (Request::SendMessage(SendMessageParams { session_id: String::new(), text: String::new(), images: Vec::new() }), "send_message"),
            (Request::SendConnectorMessage(SendConnectorMessageParams { connector: String::new(), channel_id: String::new(), author: String::new(), content: String::new(), message_id: None }), "send_connector_message"),
            (Request::CancelSession(CancelSessionParams { session_id: String::new() }), "cancel_session"),
        ];
        for (req, expected) in variants {
            assert_eq!(req.method_name(), expected);
        }
    }
}
