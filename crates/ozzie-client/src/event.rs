/// Typed events delivered to client consumers.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    Connected {
        session_id: String,
    },
    StreamDelta {
        content: String,
    },
    MessageComplete {
        content: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        call_id: String,
        result: String,
        is_error: bool,
    },
    PromptRequest {
        token: String,
        prompt_type: String,
        label: String,
    },
    SkillEvent {
        event_type: String,
        skill: String,
    },
    Error {
        message: String,
    },
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_debug() {
        let event = ClientEvent::StreamDelta {
            content: "hello".to_string(),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("StreamDelta"));
    }

    #[test]
    fn event_clone() {
        let event = ClientEvent::Connected {
            session_id: "sess_test".to_string(),
        };
        let cloned = event.clone();
        if let ClientEvent::Connected { session_id } = cloned {
            assert_eq!(session_id, "sess_test");
        } else {
            panic!("expected Connected variant");
        }
    }
}
