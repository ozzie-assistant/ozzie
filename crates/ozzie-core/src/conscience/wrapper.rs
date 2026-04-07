use std::sync::Arc;

use crate::domain::{Tool, ToolError, ToolInfo, TOOL_CTX};
use crate::events::{Event, EventBus, EventPayload, EventSource, PromptOption};

use super::permissions::ToolPermissions;

/// Response from the user approval flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalResponse {
    /// Allow this single invocation.
    AllowOnce,
    /// Allow for the entire session.
    AllowSession,
    /// Deny execution.
    Deny,
}

impl ApprovalResponse {
    /// Wire value used in events and WS frames.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AllowOnce => "once",
            Self::AllowSession => "session",
            Self::Deny => "deny",
        }
    }

    /// Parse from wire value. Unknown values map to `Deny`.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "once" => Self::AllowOnce,
            "session" => Self::AllowSession,
            _ => Self::Deny,
        }
    }

    /// Display label for UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::AllowOnce => "Allow once",
            Self::AllowSession => "Always allow for this session",
            Self::Deny => "Deny",
        }
    }

    /// All variants in display order.
    pub const ALL: [ApprovalResponse; 3] = [
        Self::AllowOnce,
        Self::AllowSession,
        Self::Deny,
    ];

    /// Build `PromptOption` list for approval flows.
    pub fn prompt_options() -> Vec<PromptOption> {
        Self::ALL
            .iter()
            .map(|r| PromptOption {
                value: r.as_str().to_string(),
                label: r.label().to_string(),
            })
            .collect()
    }
}

/// Callback to request approval from the user.
/// Receives (session_id, tool_name, arguments) and returns the approval response.
#[async_trait::async_trait]
pub trait ApprovalRequester: Send + Sync {
    async fn request_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        arguments: &str,
    ) -> Result<ApprovalResponse, ToolError>;
}

/// Wraps a tool with dangerous-tool approval flow.
///
/// Before executing the inner tool, checks permissions and optionally
/// prompts the user for approval. The session ID is read from TOOL_CTX
/// at runtime, so the wrapper can be shared across sessions.
pub struct DangerousToolWrapper {
    inner: Arc<dyn Tool>,
    tool_name: String,
    permissions: Arc<ToolPermissions>,
    bus: Arc<dyn EventBus>,
    approver: Arc<dyn ApprovalRequester>,
}

impl DangerousToolWrapper {
    pub fn new(
        inner: Arc<dyn Tool>,
        tool_name: &str,
        permissions: Arc<ToolPermissions>,
        bus: Arc<dyn EventBus>,
        approver: Arc<dyn ApprovalRequester>,
    ) -> Self {
        Self {
            inner,
            tool_name: tool_name.to_string(),
            permissions,
            bus,
            approver,
        }
    }

    /// Wraps a tool if it is dangerous; returns the original tool otherwise.
    pub fn wrap_if_dangerous(
        tool: Arc<dyn Tool>,
        tool_name: &str,
        dangerous: bool,
        permissions: Arc<ToolPermissions>,
        bus: Arc<dyn EventBus>,
        approver: Arc<dyn ApprovalRequester>,
    ) -> Arc<dyn Tool> {
        if !dangerous {
            return tool;
        }
        Arc::new(Self::new(tool, tool_name, permissions, bus, approver))
    }
}

/// Truncates a string at a given byte length, respecting UTF-8 boundaries.
pub fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Builds a prompt label for tool approval with truncated arguments.
pub fn prompt_label(tool_name: &str, arguments: &str) -> String {
    let truncated = truncate_str(arguments, 200);
    let suffix = if arguments.len() > 200 { "…" } else { "" };
    format!(
        "Tool \"{tool_name}\" requires approval. Arguments: {truncated}{suffix}"
    )
}

#[async_trait::async_trait]
impl Tool for DangerousToolWrapper {
    fn info(&self) -> ToolInfo {
        self.inner.info()
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        // Get session_id from task-local context
        let session_id = TOOL_CTX
            .try_with(|ctx| ctx.session_id.clone())
            .unwrap_or_default();

        if session_id.is_empty() {
            // No session context — cannot prompt, deny by default
            return Err(ToolError::Execution(format!(
                "tool '{}' requires approval but no session context is available",
                self.tool_name
            )));
        }

        // Check if already approved
        if self.permissions.is_allowed(&session_id, &self.tool_name) {
            return self.inner.run(arguments_json).await;
        }

        // Request approval
        let response = self
            .approver
            .request_approval(&session_id, &self.tool_name, arguments_json)
            .await?;

        match response {
            ApprovalResponse::AllowOnce => {
                self.bus.publish(Event::with_session(
                    EventSource::Agent,
                    EventPayload::ToolApproved {
                        tool: self.tool_name.clone(),
                        decision: "allow_once".to_string(),
                    },
                    &session_id,
                ));
                self.inner.run(arguments_json).await
            }
            ApprovalResponse::AllowSession => {
                self.permissions
                    .allow_for_session(&session_id, &self.tool_name);
                self.bus.publish(Event::with_session(
                    EventSource::Agent,
                    EventPayload::ToolApproved {
                        tool: self.tool_name.clone(),
                        decision: "allow_session".to_string(),
                    },
                    &session_id,
                ));
                self.inner.run(arguments_json).await
            }
            ApprovalResponse::Deny => Err(ToolError::Execution(format!(
                "tool '{}' execution denied by user",
                self.tool_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ToolContext;
    use crate::events::{Bus, EventKind};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockTool {
        call_count: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn info(&self) -> ToolInfo {
            ToolInfo::new("mock_cmd", "mock dangerous tool")
        }
        async fn run(&self, _args: &str) -> Result<String, ToolError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok("executed".to_string())
        }
    }

    struct AlwaysAllowApprover;

    #[async_trait::async_trait]
    impl ApprovalRequester for AlwaysAllowApprover {
        async fn request_approval(
            &self,
            _session_id: &str,
            _tool_name: &str,
            _arguments: &str,
        ) -> Result<ApprovalResponse, ToolError> {
            Ok(ApprovalResponse::AllowOnce)
        }
    }

    struct SessionApprover;

    #[async_trait::async_trait]
    impl ApprovalRequester for SessionApprover {
        async fn request_approval(
            &self,
            _session_id: &str,
            _tool_name: &str,
            _arguments: &str,
        ) -> Result<ApprovalResponse, ToolError> {
            Ok(ApprovalResponse::AllowSession)
        }
    }

    struct DenyApprover;

    #[async_trait::async_trait]
    impl ApprovalRequester for DenyApprover {
        async fn request_approval(
            &self,
            _session_id: &str,
            _tool_name: &str,
            _arguments: &str,
        ) -> Result<ApprovalResponse, ToolError> {
            Ok(ApprovalResponse::Deny)
        }
    }

    fn make_wrapper(approver: Arc<dyn ApprovalRequester>) -> DangerousToolWrapper {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        let tool = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });
        DangerousToolWrapper::new(tool, "mock_cmd", perms, bus, approver)
    }

    #[tokio::test]
    async fn allow_once_executes() {
        let wrapper = make_wrapper(Arc::new(AlwaysAllowApprover));
        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;
        assert_eq!(result.unwrap(), "executed");
    }

    #[tokio::test]
    async fn allow_session_persists() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        let tool = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });
        let wrapper = DangerousToolWrapper::new(
            tool.clone(),
            "mock_cmd",
            perms.clone(),
            bus,
            Arc::new(SessionApprover),
        );

        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;
        assert_eq!(result.unwrap(), "executed");

        // Approval should now be persisted
        assert!(perms.is_allowed("s1", "mock_cmd"));
    }

    #[tokio::test]
    async fn deny_blocks_execution() {
        let wrapper = make_wrapper(Arc::new(DenyApprover));
        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("denied by user"));
    }

    #[tokio::test]
    async fn pre_approved_skips_prompt() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        perms.allow_for_session("s1", "mock_cmd");

        let tool = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });
        // Using DenyApprover — should never be called because pre-approved
        let wrapper = DangerousToolWrapper::new(
            tool.clone(),
            "mock_cmd",
            perms,
            bus,
            Arc::new(DenyApprover),
        );

        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;
        assert_eq!(result.unwrap(), "executed");
        assert_eq!(tool.call_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn globally_allowed_skips_prompt() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec!["mock_cmd".to_string()]));
        let tool = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });
        let wrapper = DangerousToolWrapper::new(
            tool.clone(),
            "mock_cmd",
            perms,
            bus,
            Arc::new(DenyApprover),
        );

        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let result = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;
        assert_eq!(result.unwrap(), "executed");
    }

    #[tokio::test]
    async fn no_session_context_denies() {
        let wrapper = make_wrapper(Arc::new(AlwaysAllowApprover));
        // No TOOL_CTX set — should fail
        let result = wrapper.run("{}").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no session context"));
    }

    #[tokio::test]
    async fn wrap_if_dangerous_wraps() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        let tool: Arc<dyn Tool> = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });

        let wrapped =
            DangerousToolWrapper::wrap_if_dangerous(tool, "t", true, perms, bus, Arc::new(DenyApprover));
        // Should be a DangerousToolWrapper — test by running without context
        let result = wrapped.run("{}").await;
        assert!(result.is_err()); // no session context
    }

    #[tokio::test]
    async fn wrap_if_not_dangerous_passes_through() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        let tool: Arc<dyn Tool> = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });

        let wrapped = DangerousToolWrapper::wrap_if_dangerous(
            tool,
            "t",
            false,
            perms,
            bus,
            Arc::new(DenyApprover),
        );
        // Should pass through — no wrapping, runs directly
        let result = wrapped.run("{}").await;
        assert_eq!(result.unwrap(), "executed");
    }

    #[tokio::test]
    async fn emits_tool_approved_event() {
        let bus = Arc::new(Bus::new(64));
        let perms = Arc::new(ToolPermissions::new(vec![]));
        let tool = Arc::new(MockTool {
            call_count: AtomicUsize::new(0),
        });
        let mut rx = bus.subscribe(&[EventKind::ToolApproved.as_str()]);

        let wrapper = DangerousToolWrapper::new(
            tool,
            "mock_cmd",
            perms,
            bus,
            Arc::new(SessionApprover),
        );

        let ctx = ToolContext {
            session_id: "s1".to_string(),
            ..Default::default()
        };
        let _ = TOOL_CTX
            .scope(ctx, async { wrapper.run("{}").await })
            .await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type(), "tool.approved");
        match &event.payload {
            EventPayload::ToolApproved { tool, decision } => {
                assert_eq!(tool, "mock_cmd");
                assert_eq!(decision, "allow_session");
            }
            _ => panic!("expected ToolApproved"),
        }
        assert_eq!(event.session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn truncate_str_basic() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn prompt_label_format() {
        let label = prompt_label("execute", "{\"command\": \"ls\"}");
        assert!(label.contains("execute"));
        assert!(label.contains("requires approval"));
        assert!(label.contains("ls"));
    }
}
