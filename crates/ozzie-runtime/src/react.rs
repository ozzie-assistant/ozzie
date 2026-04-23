use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ozzie_core::domain::{Message, ProgressSender, Tool, ToolContext, ToolError, TOOL_CTX};
use ozzie_llm::{ChatMessage, ChatResponse, ChatRole, LlmError, Provider, ToolCall, ToolDefinition};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

// ---- Tool-response compaction ----

/// Maximum characters for a tool response before truncation (~3000 tokens).
const TOOL_RESPONSE_MAX_CHARS: usize = 12_000;

/// Truncates a tool response that exceeds [`TOOL_RESPONSE_MAX_CHARS`].
///
/// Keeps the first ~70% and last ~10% of the budget, inserting a truncation
/// marker in between so the LLM knows data was omitted.
fn compact_tool_response(result: &str, tool_name: &str) -> String {
    if result.len() <= TOOL_RESPONSE_MAX_CHARS {
        return result.to_string();
    }

    let total_lines = result.lines().count();
    let head_budget = (TOOL_RESPONSE_MAX_CHARS as f64 * 0.7) as usize;
    let tail_budget = (TOOL_RESPONSE_MAX_CHARS as f64 * 0.1) as usize;

    // Find a safe UTF-8 boundary for head
    let head_end = result
        .char_indices()
        .take_while(|(i, _)| *i < head_budget)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let head = &result[..head_end];

    // Find a safe UTF-8 boundary for tail
    let tail_start_raw = result.len().saturating_sub(tail_budget);
    let tail_start = result[tail_start_raw..]
        .char_indices()
        .next()
        .map(|(i, _)| tail_start_raw + i)
        .unwrap_or(result.len());
    let tail = &result[tail_start..];

    let head_lines = head.lines().count();
    let tail_lines = tail.lines().count();
    let omitted = total_lines.saturating_sub(head_lines + tail_lines);

    debug!(
        tool = tool_name,
        original_len = result.len(),
        omitted_lines = omitted,
        "tool response truncated"
    );

    format!(
        "{head}\n\n... [{omitted} lines truncated, {total_lines} total] ...\n\n{tail}"
    )
}

// ---- Traits ----

/// Observer for ReactLoop events (streaming, tool calls, LLM calls).
///
/// Implemented by the EventRunner to bridge loop events to the event bus.
/// All methods have default no-op implementations for simpler callers.
#[async_trait::async_trait]
pub trait ReactObserver: Send + Sync {
    /// Called after each LLM response with token usage.
    fn on_llm_response(&self, _input_tokens: u64, _output_tokens: u64) {}
    /// Called when the LLM produces a content delta (non-empty response text).
    fn on_stream_delta(&self, _content: &str, _index: u64) {}
    /// Called before executing a tool call.
    fn on_tool_call(&self, _call_id: &str, _tool: &str, _arguments: &str) {}
    /// Called after a tool call completes.
    fn on_tool_result(&self, _call_id: &str, _tool: &str, _result: &str, _is_error: bool) {}
    /// Called before tool execution for side effects (e.g. connector reactions).
    async fn on_tool_pre_execute(&self, _tool: &str) {}
    /// Called when a pending user message is drained and injected into the conversation.
    async fn on_pending_drained(&self, _text: &str) {}
    /// Returns a progress sender for long-running tools.
    fn progress_sender(&self) -> Option<ProgressSender> {
        None
    }
}

/// Source for pending user messages buffered during the loop.
pub trait PendingDrain: Send + Sync {
    /// Drains and returns all pending messages. Returns empty vec if none.
    fn drain(&self) -> Vec<String>;
}

// ---- Budget ----

/// Budget for a single ReAct turn.
///
/// The loop stops at the **first** limit reached:
/// - `max_turns`: hard cap on LLM calls (safety net)
/// - `max_output_tokens`: cumulative output token budget (cost control)
/// - `timeout`: wall clock time
#[derive(Debug, Clone)]
pub struct TurnBudget {
    /// Hard cap on LLM calls. Default: 50.
    pub max_turns: u32,
    /// Cumulative output token budget. Default: 32_000. 0 = unlimited.
    pub max_output_tokens: u64,
    /// Wall clock timeout. Default: 5 min.
    pub timeout: Duration,
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_output_tokens: 32_000,
            timeout: Duration::from_secs(300),
        }
    }
}

impl TurnBudget {
    /// Applies overrides from a config BudgetConfig. Unset fields keep defaults.
    pub fn with_config_overrides(
        mut self,
        config: &ozzie_core::config::BudgetConfig,
    ) -> Self {
        if let Some(turns) = config.max_turns {
            self.max_turns = turns;
        }
        if let Some(tokens) = config.max_output_tokens {
            self.max_output_tokens = tokens;
        }
        if let Some(secs) = config.timeout_sec {
            self.timeout = Duration::from_secs(secs);
        }
        self
    }

    /// Budget for subtasks: lower token budget, shorter timeout.
    pub fn subtask() -> Self {
        Self {
            max_turns: 30,
            max_output_tokens: 16_000,
            timeout: Duration::from_secs(180),
        }
    }

    /// Budget for scheduled tasks: generous.
    pub fn scheduled() -> Self {
        Self {
            max_turns: 50,
            max_output_tokens: 32_000,
            timeout: Duration::from_secs(300),
        }
    }
}

// ---- Repetition detection ----

/// Tracks recent tool-call fingerprints to detect looping behaviour.
///
/// When the same (tool_name, arguments) combination appears `threshold` times
/// within the last `window_size` calls, the loop injects a warning message
/// asking the LLM to change strategy.
struct RepetitionTracker {
    window_size: usize,
    threshold: usize,
    fingerprints: Vec<u64>,
}

impl RepetitionTracker {
    fn new(window_size: usize, threshold: usize) -> Self {
        Self {
            window_size,
            threshold,
            fingerprints: Vec::with_capacity(window_size),
        }
    }

    /// Records a tool call. Returns `true` if the repetition threshold is hit.
    fn record(&mut self, tool_name: &str, arguments: &serde_json::Value) -> bool {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tool_name.hash(&mut hasher);
        arguments.to_string().hash(&mut hasher);
        let fp = hasher.finish();

        self.fingerprints.push(fp);
        if self.fingerprints.len() > self.window_size {
            self.fingerprints.remove(0);
        }

        self.fingerprints.iter().filter(|&&f| f == fp).count() >= self.threshold
    }
}

// ---- Config ----

/// Configuration for the ReAct loop.
pub struct ReactConfig {
    /// LLM provider to use.
    pub provider: Arc<dyn Provider>,
    /// Available tools for this turn.
    pub tools: Vec<Arc<dyn Tool>>,
    /// System instruction (prepended as system message when using `run_from_messages`).
    pub instruction: String,
    /// Turn budget.
    pub budget: TurnBudget,
    /// Optional observer for event emission (streaming, tool calls, etc.).
    pub observer: Option<Arc<dyn ReactObserver>>,
    /// Conversation ID for TOOL_CTX injection. When set, tools run with context.
    pub conversation_id: Option<String>,
    /// Working directory for TOOL_CTX.
    pub work_dir: Option<String>,
    /// Optional source for pending user messages (drained before each LLM call).
    pub pending_source: Option<Arc<dyn PendingDrain>>,
    /// Optional cancellation token — checked between turns.
    pub cancel_token: Option<CancellationToken>,
    /// Sliding window size for repetition detection (0 = disabled). Default: 10.
    pub repetition_window: usize,
    /// How many identical fingerprints in the window trigger a warning. Default: 3.
    pub repetition_threshold: usize,
    /// Auto-commit file writes in the current workspace.
    pub git_auto_commit: bool,
}

impl Default for ReactConfig {
    fn default() -> Self {
        Self {
            provider: Arc::new(NoopProvider),
            tools: Vec::new(),
            instruction: String::new(),
            budget: TurnBudget::default(),
            observer: None,
            conversation_id: None,
            work_dir: None,
            pending_source: None,
            cancel_token: None,
            repetition_window: 10,
            repetition_threshold: 3,
            git_auto_commit: false,
        }
    }
}

// ---- Result ----

/// Metrics from a ReAct loop execution.
#[derive(Debug)]
pub struct TurnResult {
    /// Final assistant response text.
    pub content: String,
    /// Total tool calls made during this turn.
    pub tool_calls_count: usize,
    /// Total LLM calls made during this turn.
    pub llm_calls_count: usize,
    /// Total output tokens consumed.
    pub output_tokens_used: u64,
}

/// Outcome of a ReactLoop execution.
#[derive(Debug)]
pub enum ReactResult {
    /// LLM produced a final response (no more tool calls).
    Completed(TurnResult),
    /// Budget exhausted (turns, tokens, or timeout).
    BudgetExhausted(TurnResult),
    /// Cancelled by user (ctrl+c, /stop).
    Cancelled {
        turns: usize,
        reason: String,
    },
    /// LLM voluntarily yielded via yield_control tool.
    Yielded {
        turns: usize,
        reason: String,
        resume_on: Option<String>,
    },
    /// Error during execution.
    Error(ReactError),
}

impl ReactResult {
    /// Returns the final content, regardless of outcome.
    pub fn content(&self) -> &str {
        match self {
            Self::Completed(r) | Self::BudgetExhausted(r) => &r.content,
            Self::Cancelled { .. } | Self::Yielded { .. } | Self::Error(_) => "",
        }
    }

    /// Returns true if the loop completed normally.
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}

// ---- Loop ----

/// Direct ReAct loop: chat -> tool calls -> chat, no framework.
///
/// Takes a conversation and runs the LLM in a tool-calling loop
/// until the LLM produces a final text response or a budget limit is reached.
pub struct ReactLoop;

impl ReactLoop {
    /// Runs the loop from pre-built ChatMessages (used by EventRunner).
    pub async fn run(
        config: &ReactConfig,
        chat_messages: Vec<ChatMessage>,
    ) -> ReactResult {
        Self::run_inner(config, chat_messages).await
    }

    /// Runs the loop from domain Messages (builds ChatMessages from instruction + messages).
    pub async fn run_from_messages(
        config: &ReactConfig,
        messages: Vec<Message>,
    ) -> ReactResult {
        let chat_messages = build_chat_messages(&config.instruction, &messages);
        Self::run_inner(config, chat_messages).await
    }

    async fn run_inner(
        config: &ReactConfig,
        mut chat_messages: Vec<ChatMessage>,
    ) -> ReactResult {
        let tool_defs = build_tool_definitions(&config.tools);
        let tool_map: HashMap<String, &Arc<dyn Tool>> = config
            .tools
            .iter()
            .map(|t| (t.info().name.clone(), t))
            .collect();

        let mut tool_calls_count = 0usize;
        let mut llm_calls_count = 0usize;
        let mut output_tokens_used = 0u64;
        let mut index = 0u64;
        let budget = &config.budget;

        let mut repetition_tracker = if config.repetition_window > 0 {
            Some(RepetitionTracker::new(config.repetition_window, config.repetition_threshold))
        } else {
            None
        };

        let deadline = tokio::time::Instant::now() + budget.timeout;

        loop {
            // Drain pending user messages before each LLM call
            if let Some(ref source) = config.pending_source {
                for text in source.drain() {
                    if let Some(ref obs) = config.observer {
                        obs.on_pending_drained(&text).await;
                    }
                    chat_messages.push(ChatMessage::text(ChatRole::User, text));
                }
            }

            // Check cancellation
            if let Some(ref token) = config.cancel_token
                && token.is_cancelled()
            {
                info!(turns = llm_calls_count, "react loop: cancelled");
                return ReactResult::Cancelled {
                    turns: llm_calls_count,
                    reason: "user_request".to_string(),
                };
            }

            // Check turn limit
            if llm_calls_count >= budget.max_turns as usize {
                warn!(
                    turns = llm_calls_count,
                    max = budget.max_turns,
                    tokens = output_tokens_used,
                    "react loop: max turns reached"
                );
                return ReactResult::BudgetExhausted(TurnResult {
                    content: format!(
                        "[budget exhausted: {} turns, {} output tokens]",
                        llm_calls_count, output_tokens_used
                    ),
                    tool_calls_count,
                    llm_calls_count,
                    output_tokens_used,
                });
            }

            // Check token budget
            if budget.max_output_tokens > 0 && output_tokens_used >= budget.max_output_tokens {
                warn!(
                    tokens = output_tokens_used,
                    limit = budget.max_output_tokens,
                    turns = llm_calls_count,
                    "react loop: output token budget exhausted"
                );
                return ReactResult::BudgetExhausted(TurnResult {
                    content: format!(
                        "[budget exhausted: {} turns, {} output tokens]",
                        llm_calls_count, output_tokens_used
                    ),
                    tool_calls_count,
                    llm_calls_count,
                    output_tokens_used,
                });
            }

            debug!(
                turn = llm_calls_count,
                tokens = output_tokens_used,
                "react loop: calling LLM"
            );

            // Call LLM with remaining timeout
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return ReactResult::Error(ReactError::Timeout);
            }

            let response = match tokio::time::timeout(
                remaining,
                config.provider.chat(&chat_messages, &tool_defs),
            )
            .await
            {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return ReactResult::Error(ReactError::Llm(e)),
                Err(_) => return ReactResult::Error(ReactError::Timeout),
            };

            llm_calls_count += 1;
            output_tokens_used += response.usage.output_tokens;

            // Notify observer of LLM call
            if let Some(ref obs) = config.observer {
                obs.on_llm_response(response.usage.input_tokens, response.usage.output_tokens);
            }

            // No tool calls -> final response
            if response.tool_calls.is_empty() {
                if let Some(ref obs) = config.observer
                    && !response.content.is_empty()
                {
                    index += 1;
                    obs.on_stream_delta(&response.content, index);
                }

                info!(
                    turns = llm_calls_count,
                    tool_calls = tool_calls_count,
                    output_tokens = output_tokens_used,
                    "react loop: completed"
                );
                return ReactResult::Completed(TurnResult {
                    content: response.content,
                    tool_calls_count,
                    llm_calls_count,
                    output_tokens_used,
                });
            }

            // Process tool calls
            chat_messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: ozzie_llm::text_to_parts(&response.content),
                tool_calls: response.tool_calls.clone(),
                tool_call_id: None,
            });

            for tc in &response.tool_calls {
                tool_calls_count += 1;

                // Detect yield_control tool — short-circuit the loop
                if tc.name == "yield_control" {
                    let args_str = tc.arguments.to_string();
                    let (reason, resume_on) = parse_yield_args(&args_str);
                    info!(turns = llm_calls_count, reason = %reason, "react loop: yielded");
                    return ReactResult::Yielded {
                        turns: llm_calls_count,
                        reason,
                        resume_on,
                    };
                }

                // Notify observer before execution
                if let Some(ref obs) = config.observer {
                    let args_str = tc.arguments.to_string();
                    obs.on_tool_call(&tc.id, &tc.name, &args_str);
                    obs.on_tool_pre_execute(&tc.name).await;
                }

                let result = execute_tool_call(
                    &tool_map,
                    tc,
                    config.conversation_id.as_deref(),
                    config.work_dir.as_deref(),
                    config.observer.as_ref(),
                    config.git_auto_commit,
                )
                .await;

                let is_error = result.starts_with("Error:");

                // Notify observer after execution (with full result for logging)
                if let Some(ref obs) = config.observer {
                    obs.on_tool_result(&tc.id, &tc.name, &result, is_error);
                }

                // Compact large tool responses to preserve context budget
                let result = compact_tool_response(&result, &tc.name);

                chat_messages.push(ChatMessage {
                    role: ChatRole::Tool,
                    content: ozzie_llm::text_to_parts(result),
                    tool_calls: Vec::new(),
                    tool_call_id: Some(tc.id.clone()),
                });

                // Repetition detection: warn the LLM if it keeps calling the same tool
                if let Some(ref mut tracker) = repetition_tracker
                    && tracker.record(&tc.name, &tc.arguments)
                {
                    warn!(tool = %tc.name, "repetition detected: agent is looping");
                    chat_messages.push(ChatMessage::text(
                        ChatRole::User,
                        "You are repeating the same actions with the same arguments. \
                         Try a different approach or explain what you are stuck on.",
                    ));
                    break;
                }
            }
        }
    }
}

/// Executes a single tool call with optional TOOL_CTX scope.
async fn execute_tool_call(
    tool_map: &HashMap<String, &Arc<dyn Tool>>,
    tc: &ToolCall,
    conversation_id: Option<&str>,
    work_dir: Option<&str>,
    observer: Option<&Arc<dyn ReactObserver>>,
    git_auto_commit: bool,
) -> String {
    let resolved_name = resolve_tool_name(&tc.name, tool_map);
    let raw = match tool_map.get(&resolved_name) {
        Some(tool) => {
            let tool = Arc::clone(tool);
            let args_str = tc.arguments.to_string();

            if let Some(sid) = conversation_id {
                // Build progress sender from observer
                let progress: Option<ProgressSender> =
                    observer.and_then(|obs| obs.progress_sender());

                let ctx = ToolContext {
                    conversation_id: sid.to_string(),
                    work_dir: work_dir.map(|s| s.to_string()),
                    progress,
                    git_auto_commit,
                    ..Default::default()
                };
                match TOOL_CTX.scope(ctx, async move { tool.run(&args_str).await }).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {e}"),
                }
            } else {
                // No session context — run tool directly
                match tool.run(&args_str).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {e}"),
                }
            }
        }
        None => format!("Error: tool '{}' not found", tc.name),
    };
    ozzie_core::conscience::scrub_credentials(&raw)
}

/// Resolves a tool name through exact match, then normalized form (lowercase,
/// dashes→underscores). This compensates for LLMs that produce slight name
/// variations (e.g. `"Shell-Exec"` instead of `"shell_exec"`).
fn resolve_tool_name(name: &str, tool_map: &HashMap<String, &Arc<dyn Tool>>) -> String {
    if tool_map.contains_key(name) {
        return name.to_string();
    }
    let normalized = name.trim().replace('-', "_").to_ascii_lowercase();
    if tool_map.contains_key(&normalized) {
        return normalized;
    }
    // No match — return original so the caller produces a clean error.
    name.to_string()
}

/// Parses yield_control arguments to extract reason and optional resume_on.
fn parse_yield_args(arguments: &str) -> (String, Option<String>) {
    #[derive(serde::Deserialize)]
    struct YieldArgs {
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        resume_on: Option<String>,
    }

    match serde_json::from_str::<YieldArgs>(arguments) {
        Ok(args) => (
            args.reason.unwrap_or_else(|| "done".to_string()),
            args.resume_on,
        ),
        Err(_) => ("done".to_string(), None),
    }
}

/// Builds LLM-compatible tool definitions from Tool trait objects.
pub fn build_tool_definitions(tools: &[Arc<dyn Tool>]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|t| {
            let info = t.info();
            ToolDefinition {
                name: info.name,
                description: info.description,
                parameters: info.parameters,
            }
        })
        .collect()
}

/// Converts domain Messages to LLM ChatMessages.
fn build_chat_messages(instruction: &str, messages: &[Message]) -> Vec<ChatMessage> {
    let mut chat = Vec::new();

    if !instruction.is_empty() {
        chat.push(ChatMessage::text(ChatRole::System, instruction));
    }

    for msg in messages {
        if !msg.agent_visible {
            continue;
        }
        let role = match msg.role.as_str() {
            "system" => ChatRole::System,
            "assistant" => ChatRole::Assistant,
            "tool" => ChatRole::Tool,
            _ => ChatRole::User,
        };
        chat.push(ChatMessage::text(role, &msg.content));
    }

    chat
}

#[derive(Debug, thiserror::Error)]
pub enum ReactError {
    #[error("LLM error: {0}")]
    Llm(#[from] LlmError),
    #[error("tool execution failed: {0}")]
    Tool(#[from] ToolError),
    #[error("timeout")]
    Timeout,
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
}

/// Noop provider for default config.
struct NoopProvider;

#[async_trait::async_trait]
impl Provider for NoopProvider {
    async fn chat(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ChatResponse, LlmError> {
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: Vec::new(),
            usage: ozzie_llm::TokenUsage::default(),
            stop_reason: None,
            model: None,
        })
    }

    async fn chat_stream(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<
        std::pin::Pin<
            Box<dyn futures_core::Stream<Item = Result<ozzie_llm::ChatDelta, LlmError>> + Send>,
        >,
        LlmError,
    > {
        Err(LlmError::Other("noop provider".to_string()))
    }

    fn name(&self) -> &str {
        "noop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ozzie_core::domain::ToolInfo;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock provider that returns a fixed response then stops.
    struct MockProvider {
        responses: Vec<ChatResponse>,
        call_count: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<ChatResponse, LlmError> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            if idx < self.responses.len() {
                Ok(self.responses[idx].clone())
            } else {
                Ok(ChatResponse {
                    content: "done".to_string(),
                    tool_calls: Vec::new(),
                    usage: ozzie_llm::TokenUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        ..Default::default()
                    },
                    stop_reason: None,
                    model: None,
                })
            }
        }

        async fn chat_stream(
            &self,
            _: &[ChatMessage],
            _: &[ToolDefinition],
        ) -> Result<
            std::pin::Pin<
                Box<
                    dyn futures_core::Stream<Item = Result<ozzie_llm::ChatDelta, LlmError>>
                        + Send,
                >,
            >,
            LlmError,
        > {
            Err(LlmError::Other("not implemented".to_string()))
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn info(&self) -> ToolInfo {
            ToolInfo::new("echo", "Echoes input")
        }

        async fn run(&self, args: &str) -> Result<String, ToolError> {
            Ok(format!("echo: {args}"))
        }
    }

    #[tokio::test]
    async fn simple_response_no_tools() {
        let provider = Arc::new(MockProvider {
            responses: vec![ChatResponse {
                content: "Hello!".to_string(),
                tool_calls: Vec::new(),
                usage: ozzie_llm::TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            }],
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: Vec::new(),
            instruction: "You are helpful.".to_string(),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("hi")]).await;
        let result = match result {
            ReactResult::Completed(r) => r,
            other => panic!("expected Completed, got: {other:?}"),
        };

        assert_eq!(result.content, "Hello!");
        assert_eq!(result.tool_calls_count, 0);
        assert_eq!(result.llm_calls_count, 1);
        assert_eq!(result.output_tokens_used, 5);
    }

    #[tokio::test]
    async fn tool_call_then_response() {
        let provider = Arc::new(MockProvider {
            responses: vec![
                ChatResponse {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "call_0".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({"text": "test"}),
                    }],
                    usage: ozzie_llm::TokenUsage {
                        input_tokens: 10,
                        output_tokens: 50,
                        ..Default::default()
                    },
                    stop_reason: None,
                    model: None,
                },
                ChatResponse {
                    content: "Got the echo result.".to_string(),
                    tool_calls: Vec::new(),
                    usage: ozzie_llm::TokenUsage {
                        input_tokens: 20,
                        output_tokens: 10,
                        ..Default::default()
                    },
                    stop_reason: None,
                    model: None,
                },
            ],
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("use echo")]).await;
        let result = match result {
            ReactResult::Completed(r) => r,
            other => panic!("expected Completed, got: {other:?}"),
        };

        assert_eq!(result.content, "Got the echo result.");
        assert_eq!(result.tool_calls_count, 1);
        assert_eq!(result.llm_calls_count, 2);
        assert_eq!(result.output_tokens_used, 60); // 50 + 10
    }

    #[tokio::test]
    async fn token_budget_stops_loop() {
        let mut responses = Vec::new();
        for i in 0..100 {
            responses.push(ChatResponse {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: format!("call_{i}"),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({}),
                }],
                usage: ozzie_llm::TokenUsage {
                    input_tokens: 10,
                    output_tokens: 1000,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            });
        }

        let provider = Arc::new(MockProvider {
            responses,
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            budget: TurnBudget {
                max_turns: 100,
                max_output_tokens: 3000,
                timeout: Duration::from_secs(10),
            },
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("loop")]).await;
        assert!(
            matches!(result, ReactResult::BudgetExhausted(_)),
            "expected BudgetExhausted, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn observer_receives_events() {
        use std::sync::Mutex;

        #[derive(Default)]
        struct TestObserver {
            llm_calls: AtomicUsize,
            tool_calls: Mutex<Vec<String>>,
            deltas: Mutex<Vec<String>>,
        }

        #[async_trait::async_trait]
        impl ReactObserver for TestObserver {
            fn on_llm_response(&self, _input: u64, _output: u64) {
                self.llm_calls.fetch_add(1, Ordering::Relaxed);
            }
            fn on_tool_call(&self, _call_id: &str, tool: &str, _args: &str) {
                self.tool_calls.lock().unwrap().push(tool.to_string());
            }
            fn on_stream_delta(&self, content: &str, _index: u64) {
                self.deltas.lock().unwrap().push(content.to_string());
            }
        }

        let provider = Arc::new(MockProvider {
            responses: vec![
                ChatResponse {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "c1".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({}),
                    }],
                    usage: ozzie_llm::TokenUsage { input_tokens: 5, output_tokens: 10, ..Default::default() },
                    stop_reason: None,
                    model: None,
                },
                ChatResponse {
                    content: "Final answer".to_string(),
                    tool_calls: Vec::new(),
                    usage: ozzie_llm::TokenUsage { input_tokens: 15, output_tokens: 20, ..Default::default() },
                    stop_reason: None,
                    model: None,
                },
            ],
            call_count: AtomicUsize::new(0),
        });

        let observer = Arc::new(TestObserver::default());
        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            observer: Some(observer.clone()),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("go")]).await;
        assert!(result.is_completed());

        assert_eq!(observer.llm_calls.load(Ordering::Relaxed), 2);
        assert_eq!(*observer.tool_calls.lock().unwrap(), vec!["echo"]);
        assert_eq!(*observer.deltas.lock().unwrap(), vec!["Final answer"]);
    }

    #[tokio::test]
    async fn pending_drain_injects_messages() {
        use std::sync::Mutex;

        struct TestDrain {
            messages: Mutex<Vec<String>>,
        }
        impl PendingDrain for TestDrain {
            fn drain(&self) -> Vec<String> {
                std::mem::take(&mut *self.messages.lock().unwrap())
            }
        }

        // Provider checks messages on second call
        struct InspectProvider {
            call_count: AtomicUsize,
            second_call_messages: Mutex<Vec<String>>,
        }

        #[async_trait::async_trait]
        impl Provider for InspectProvider {
            async fn chat(
                &self,
                messages: &[ChatMessage],
                _tools: &[ToolDefinition],
            ) -> Result<ChatResponse, LlmError> {
                let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
                if idx == 0 {
                    // First call: return tool call
                    Ok(ChatResponse {
                        content: String::new(),
                        tool_calls: vec![ToolCall {
                            id: "c1".to_string(),
                            name: "echo".to_string(),
                            arguments: serde_json::json!({}),
                        }],
                        usage: ozzie_llm::TokenUsage { input_tokens: 5, output_tokens: 5, ..Default::default() },
                        stop_reason: None,
                        model: None,
                    })
                } else {
                    // Second call: capture user messages
                    let user_msgs: Vec<String> = messages
                        .iter()
                        .filter(|m| m.role == ozzie_llm::ChatRole::User)
                        .map(|m| m.text_content())
                        .collect();
                    *self.second_call_messages.lock().unwrap() = user_msgs;
                    Ok(ChatResponse {
                        content: "done".to_string(),
                        tool_calls: Vec::new(),
                        usage: ozzie_llm::TokenUsage { input_tokens: 10, output_tokens: 5, ..Default::default() },
                        stop_reason: None,
                        model: None,
                    })
                }
            }

            async fn chat_stream(
                &self,
                _: &[ChatMessage],
                _: &[ToolDefinition],
            ) -> Result<
                std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<ozzie_llm::ChatDelta, LlmError>> + Send>>,
                LlmError,
            > {
                Err(LlmError::Other("not implemented".to_string()))
            }

            fn name(&self) -> &str {
                "inspect"
            }
        }

        let drain = Arc::new(TestDrain {
            messages: Mutex::new(vec!["buffered msg 1".to_string(), "buffered msg 2".to_string()]),
        });
        let provider = Arc::new(InspectProvider {
            call_count: AtomicUsize::new(0),
            second_call_messages: Mutex::new(Vec::new()),
        });

        let config = ReactConfig {
            provider: provider.clone(),
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            pending_source: Some(drain),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("initial")]).await;
        assert!(result.is_completed());

        // The second LLM call should have seen the buffered messages
        let seen = provider.second_call_messages.lock().unwrap();
        assert!(seen.contains(&"buffered msg 1".to_string()));
        assert!(seen.contains(&"buffered msg 2".to_string()));
        assert!(seen.contains(&"initial".to_string()));
    }

    #[tokio::test]
    async fn cancel_token_stops_loop() {
        let token = CancellationToken::new();

        // Provider returns one tool call then would continue
        let provider = Arc::new(MockProvider {
            responses: vec![
                ChatResponse {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "c1".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({}),
                    }],
                    usage: ozzie_llm::TokenUsage { input_tokens: 5, output_tokens: 5, ..Default::default() },
                    stop_reason: None,
                    model: None,
                },
            ],
            call_count: AtomicUsize::new(0),
        });

        // Cancel BEFORE the second iteration
        token.cancel();

        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            cancel_token: Some(token.clone()),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("go")]).await;
        assert!(
            matches!(result, ReactResult::Cancelled { .. }),
            "expected Cancelled, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn yield_control_stops_loop() {
        let provider = Arc::new(MockProvider {
            responses: vec![
                ChatResponse {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "c1".to_string(),
                        name: "yield_control".to_string(),
                        arguments: serde_json::json!({"reason": "done"}),
                    }],
                    usage: ozzie_llm::TokenUsage { input_tokens: 5, output_tokens: 5, ..Default::default() },
                    stop_reason: None,
                    model: None,
                },
            ],
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)], // yield_control doesn't need to be in tools
            instruction: String::new(),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("go")]).await;
        match result {
            ReactResult::Yielded { reason, resume_on, .. } => {
                assert_eq!(reason, "done");
                assert!(resume_on.is_none());
            }
            other => panic!("expected Yielded, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn yield_waiting_with_resume_on() {
        let provider = Arc::new(MockProvider {
            responses: vec![
                ChatResponse {
                    content: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "c1".to_string(),
                        name: "yield_control".to_string(),
                        arguments: serde_json::json!({"reason": "waiting", "resume_on": "task.completed"}),
                    }],
                    usage: ozzie_llm::TokenUsage { input_tokens: 5, output_tokens: 5, ..Default::default() },
                    stop_reason: None,
                    model: None,
                },
            ],
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: Vec::new(),
            instruction: String::new(),
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("go")]).await;
        match result {
            ReactResult::Yielded { reason, resume_on, .. } => {
                assert_eq!(reason, "waiting");
                assert_eq!(resume_on.as_deref(), Some("task.completed"));
            }
            other => panic!("expected Yielded, got: {other:?}"),
        }
    }

    #[test]
    fn build_chat_messages_filters_agent_invisible() {
        let messages = vec![
            Message::user("visible"),
            Message::assistant("hidden").with_agent_visible(false),
            Message::user("also visible"),
        ];
        let chat = build_chat_messages("", &messages);
        assert_eq!(chat.len(), 2);
        assert_eq!(chat[0].text_content(), "visible");
        assert_eq!(chat[1].text_content(), "also visible");
    }

    #[test]
    fn compact_short_response_unchanged() {
        let short = "Hello world";
        assert_eq!(compact_tool_response(short, "test"), short);
    }

    #[test]
    fn compact_large_response_truncated() {
        let line = "x".repeat(100);
        let lines: Vec<&str> = (0..200).map(|_| line.as_str()).collect();
        let large = lines.join("\n"); // ~20k chars
        assert!(large.len() > TOOL_RESPONSE_MAX_CHARS);

        let result = compact_tool_response(&large, "test");
        assert!(result.len() < large.len(), "should be smaller after truncation");
        assert!(result.contains("lines truncated"), "should have truncation marker");
        assert!(result.contains("200 total"), "should mention total line count");
    }

    #[test]
    fn compact_preserves_head_and_tail() {
        // Build a response with identifiable head and tail
        let mut content = String::new();
        content.push_str("HEAD_MARKER\n");
        for _ in 0..300 {
            content.push_str(&"x".repeat(100));
            content.push('\n');
        }
        content.push_str("TAIL_MARKER\n");

        let result = compact_tool_response(&content, "test");
        assert!(result.contains("HEAD_MARKER"), "head should be preserved");
        assert!(result.contains("TAIL_MARKER"), "tail should be preserved");
    }

    #[test]
    fn repetition_tracker_detects_loop() {
        let mut tracker = RepetitionTracker::new(5, 3);
        let args = serde_json::json!({"path": "/tmp/test.txt"});

        assert!(!tracker.record("file_read", &args));
        assert!(!tracker.record("file_read", &args));
        assert!(tracker.record("file_read", &args)); // 3rd hit → true
    }

    #[test]
    fn repetition_tracker_different_args_dont_trigger() {
        let mut tracker = RepetitionTracker::new(5, 3);

        assert!(!tracker.record("file_read", &serde_json::json!({"path": "a.txt"})));
        assert!(!tracker.record("file_read", &serde_json::json!({"path": "b.txt"})));
        assert!(!tracker.record("file_read", &serde_json::json!({"path": "c.txt"})));
    }

    #[test]
    fn repetition_tracker_window_slides() {
        let mut tracker = RepetitionTracker::new(4, 3);
        let same = serde_json::json!({"cmd": "ls"});

        assert!(!tracker.record("exec", &same)); // [A]
        assert!(!tracker.record("exec", &same)); // [A, A]
        // Interleave different calls to push old fingerprints out of window
        assert!(!tracker.record("other", &serde_json::json!({}))); // [A, A, B]
        assert!(!tracker.record("other2", &serde_json::json!({}))); // [A, A, B, C]
        assert!(!tracker.record("exec", &same)); // [A, B, C, A] — only 2x A in window
        assert!(!tracker.record("exec", &same)); // [B, C, A, A] — only 2x A
    }

    #[tokio::test]
    async fn repetition_injects_warning() {
        // Provider returns the same tool call every time, then finishes on 5th call
        let mut responses = Vec::new();
        for i in 0..10 {
            responses.push(ChatResponse {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: format!("c{i}"),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"same": "args"}),
                }],
                usage: ozzie_llm::TokenUsage {
                    input_tokens: 5,
                    output_tokens: 5,
                    ..Default::default()
                },
                stop_reason: None,
                model: None,
            });
        }
        // Final response after the warning
        responses.push(ChatResponse {
            content: "I'll try something different".to_string(),
            tool_calls: Vec::new(),
            usage: ozzie_llm::TokenUsage {
                input_tokens: 5,
                output_tokens: 5,
                ..Default::default()
            },
            stop_reason: None,
            model: None,
        });

        let provider = Arc::new(MockProvider {
            responses,
            call_count: AtomicUsize::new(0),
        });

        let config = ReactConfig {
            provider,
            tools: vec![Arc::new(EchoTool)],
            instruction: String::new(),
            repetition_window: 5,
            repetition_threshold: 3,
            ..Default::default()
        };

        let result = ReactLoop::run_from_messages(&config, vec![Message::user("go")]).await;
        assert!(result.is_completed(), "expected Completed, got: {result:?}");
    }
}
