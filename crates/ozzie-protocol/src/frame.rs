use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::event_kind::EventKind;
use crate::request::Request;

const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 error codes.
pub mod error_code {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    /// Generic server error (business logic errors).
    pub const SERVER_ERROR: i32 = -32000;
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 frame exchanged over WebSocket.
///
/// Discriminated by field presence:
/// - **Request**: has `id` + `method` (+ optional `params`)
/// - **Response (success)**: has `id` + `result`
/// - **Response (error)**: has `id` + `error`
/// - **Notification**: has `method` (+ optional `params`), no `id`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub jsonrpc: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

// ---- Construction ----

impl Frame {
    /// Creates a request frame from a typed `Request` enum (zero magic strings).
    pub fn from_request(id: impl Into<String>, request: &Request) -> Self {
        // Serialize the tagged enum, then extract only the "params" content.
        // The enum serializes as {"method": "...", "params": {...}}.
        let params = serde_json::to_value(request)
            .ok()
            .and_then(|v| v.get("params").cloned());
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            method: Some(request.method_name().to_string()),
            params,
            result: None,
            error: None,
        }
    }

    /// Creates a request frame with raw method + params (for callers that don't use Request enum).
    pub fn request<P: Serialize>(id: impl Into<String>, method: impl Into<String>, params: &P) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            method: Some(method.into()),
            params: serde_json::to_value(params).ok(),
            result: None,
            error: None,
        }
    }

    /// Creates a success response frame.
    pub fn response_ok<R: Serialize>(id: impl Into<String>, result: &R) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            method: None,
            params: None,
            result: serde_json::to_value(result).ok(),
            error: None,
        }
    }

    /// Creates an error response frame.
    pub fn response_err(id: impl Into<String>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(id.into()),
            method: None,
            params: None,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Creates a notification frame (server → client event, no id).
    pub fn notification<P: Serialize>(method: impl Into<String>, params: &P) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: None,
            method: Some(method.into()),
            params: serde_json::to_value(params).ok(),
            result: None,
            error: None,
        }
    }

    /// Creates a notification frame for an event with optional conversation_id injected into params.
    pub fn event<P: Serialize>(
        event_type: &str,
        conversation_id: Option<&str>,
        payload: &P,
    ) -> Self {
        let mut params = serde_json::to_value(payload).unwrap_or(serde_json::Value::Object(Default::default()));
        if let Some(sid) = conversation_id
            && let Some(obj) = params.as_object_mut()
        {
            obj.insert("conversation_id".to_string(), serde_json::Value::String(sid.to_string()));
        }
        Self::notification(event_type, &params)
    }
}

// ---- Detection ----

impl Frame {
    /// True if this frame is a request (has id + method, no result/error).
    pub fn is_request(&self) -> bool {
        self.id.is_some() && self.method.is_some() && self.result.is_none() && self.error.is_none()
    }

    /// True if this frame is a notification (has method, no id).
    pub fn is_notification(&self) -> bool {
        self.id.is_none() && self.method.is_some()
    }

    /// True if this frame is a success response.
    pub fn is_success(&self) -> bool {
        self.id.is_some() && self.result.is_some()
    }

    /// True if this frame is an error response.
    pub fn is_error(&self) -> bool {
        self.id.is_some() && self.error.is_some()
    }

    /// True if this frame is any response (success or error).
    pub fn is_response(&self) -> bool {
        self.is_success() || self.is_error()
    }
}

// ---- Parsing ----

impl Frame {
    /// Parses the request method + params into a typed `Request` enum.
    pub fn parse_request(&self) -> Result<Request, serde_json::Error> {
        // Build a JSON object with { "method": "...", "params": ... } for tagged deserialization
        let mut obj = serde_json::Map::new();
        if let Some(ref method) = self.method {
            obj.insert("method".to_string(), serde_json::Value::String(method.clone()));
        }
        if let Some(ref params) = self.params {
            obj.insert("params".to_string(), params.clone());
        }
        serde_json::from_value(serde_json::Value::Object(obj))
    }

    /// Parses the result payload into a typed struct.
    pub fn parse_result<R: DeserializeOwned>(&self) -> Result<R, serde_json::Error> {
        match &self.result {
            Some(v) => serde_json::from_value(v.clone()),
            None => serde_json::from_value(serde_json::Value::Null),
        }
    }

    /// Parses the params payload into a typed struct (for notifications/events).
    pub fn parse_params<P: DeserializeOwned>(&self) -> Result<P, serde_json::Error> {
        match &self.params {
            Some(v) => serde_json::from_value(v.clone()),
            None => serde_json::from_value(serde_json::Value::Null),
        }
    }

    /// Returns the typed `EventKind` for notification frames.
    pub fn event_kind(&self) -> Option<EventKind> {
        self.method.as_deref().and_then(EventKind::parse)
    }

    /// Returns the error message, if this is an error response.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_ref().map(|e| e.message.as_str())
    }
}

// ---- Serialization ----

impl Frame {
    /// Serializes the frame to JSON bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserializes a frame from JSON bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_types::{OpenConversationParams, SendMessageParams, ConversationResult, AcceptedResult};

    #[test]
    fn request_from_enum_roundtrip() {
        let req = Request::SendMessage(SendMessageParams {
            conversation_id: "sess_1".to_string(),
            text: "hello".to_string(),
            images: Vec::new(),
        });
        let frame = Frame::from_request("req_1", &req);
        let bytes = frame.to_bytes().unwrap();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert!(parsed.is_request());
        assert!(!parsed.is_notification());
        assert_eq!(parsed.id.as_deref(), Some("req_1"));
        assert_eq!(parsed.method.as_deref(), Some("send_message"));
        assert_eq!(parsed.jsonrpc, "2.0");

        // Parse back to Request enum
        let req_parsed = parsed.parse_request().unwrap();
        match req_parsed {
            Request::SendMessage(p) => {
                assert_eq!(p.conversation_id, "sess_1");
                assert_eq!(p.text, "hello");
            }
            _ => panic!("expected SendMessage"),
        }
    }

    #[test]
    fn request_raw_roundtrip() {
        let frame = Frame::request("req_1", "send_message", &serde_json::json!({"conversation_id": "s1", "text": "hi"}));
        let bytes = frame.to_bytes().unwrap();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert!(parsed.is_request());
        assert_eq!(parsed.method.as_deref(), Some("send_message"));
    }

    #[test]
    fn response_ok_typed() {
        let result = ConversationResult {
            conversation_id: "sess_test".to_string(),
            root_dir: Some("/tmp".to_string()),
        };
        let frame = Frame::response_ok("req_1", &result);
        let bytes = frame.to_bytes().unwrap();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert!(parsed.is_success());
        assert!(!parsed.is_error());
        assert!(!parsed.is_request());

        let r: ConversationResult = parsed.parse_result().unwrap();
        assert_eq!(r.conversation_id, "sess_test");
        assert_eq!(r.root_dir.as_deref(), Some("/tmp"));
    }

    #[test]
    fn response_err_with_code() {
        let frame = Frame::response_err("req_1", error_code::METHOD_NOT_FOUND, "unknown method: foo");
        let bytes = frame.to_bytes().unwrap();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert!(parsed.is_error());
        assert!(!parsed.is_success());
        assert_eq!(parsed.error_message(), Some("unknown method: foo"));
        assert_eq!(parsed.error.as_ref().unwrap().code, -32601);
    }

    #[test]
    fn notification_event() {
        let frame = Frame::event(
            "assistant.stream",
            Some("sess_123"),
            &serde_json::json!({"phase": "delta", "content": "Hello", "index": 1}),
        );
        let bytes = frame.to_bytes().unwrap();
        let parsed = Frame::from_bytes(&bytes).unwrap();

        assert!(parsed.is_notification());
        assert!(!parsed.is_request());
        assert_eq!(parsed.method.as_deref(), Some("assistant.stream"));
        assert_eq!(parsed.event_kind(), Some(EventKind::AssistantStream));

        // conversation_id injected into params
        let params = parsed.params.unwrap();
        assert_eq!(params["conversation_id"], "sess_123");
        assert_eq!(params["content"], "Hello");
    }

    #[test]
    fn open_session_request_from_enum() {
        let req = Request::OpenConversation(OpenConversationParams {
            working_dir: Some("/tmp".to_string()),
            ..Default::default()
        });
        let frame = Frame::from_request("1", &req);
        assert_eq!(frame.method.as_deref(), Some("open_conversation"));

        let parsed = frame.parse_request().unwrap();
        match parsed {
            Request::OpenConversation(p) => {
                assert_eq!(p.working_dir.as_deref(), Some("/tmp"));
                assert!(p.conversation_id.is_none());
            }
            _ => panic!("expected OpenSession"),
        }
    }

    #[test]
    fn accepted_result_roundtrip() {
        let frame = Frame::response_ok("1", &AcceptedResult { accepted: true });
        let parsed = Frame::from_bytes(&frame.to_bytes().unwrap()).unwrap();
        let r: AcceptedResult = parsed.parse_result().unwrap();
        assert!(r.accepted);
    }
}
