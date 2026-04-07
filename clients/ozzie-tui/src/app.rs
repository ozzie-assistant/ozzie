use chrono::Utc;

use ozzie_core::conscience::ApprovalResponse;

use crate::block::{AssistantBlock, Block, BlockId, BlockState, SystemBlock, ToolCallBlock, UserBlock};
use crate::input::InputState;
use crate::render::RenderContext;

/// Active prompt state for dangerous tool approval.
#[derive(Debug, Clone)]
pub struct ActivePrompt {
    pub token: String,
    pub label: String,
    pub prompt_type: String,
    pub selected: usize,
}

impl ActivePrompt {
    pub fn new(token: String, label: String, prompt_type: String) -> Self {
        Self {
            token,
            label,
            prompt_type,
            selected: 0,
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < ApprovalResponse::ALL.len() {
            self.selected += 1;
        }
    }

    /// Returns the response string for the currently selected option.
    pub fn response_value(&self) -> &'static str {
        ApprovalResponse::ALL[self.selected].as_str()
    }
}

/// Connection state for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Spinner animation frames.
pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Block-based TUI application model.
pub struct App {
    /// Blocks currently in the viewport (active + recently finalized).
    pub viewport_blocks: Vec<Block>,
    /// Input state.
    pub input: InputState,
    /// Connection state.
    pub connection: ConnectionState,
    /// Session ID.
    pub session_id: Option<String>,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Status message.
    pub status: String,
    /// Render context (language, etc.).
    pub render_ctx: RenderContext,
    /// Next block ID counter.
    next_block_id: u64,
    /// Index of the currently selected block (for collapse toggling).
    pub selected_block: Option<usize>,
    /// Lines scrolled up from the bottom (0 = pinned to bottom).
    pub scroll_offset: usize,
    /// Active dangerous tool approval prompt, if any.
    pub active_prompt: Option<ActivePrompt>,
    /// When the agent started working (for elapsed time display).
    pub working_since: Option<std::time::Instant>,
    /// Current spinner animation frame index.
    pub spinner_frame: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            viewport_blocks: Vec::new(),
            input: InputState::new(),
            connection: ConnectionState::Disconnected,
            session_id: None,
            should_quit: false,
            status: "Disconnected".to_string(),
            render_ctx: RenderContext::default(),
            next_block_id: 0,
            selected_block: None,
            scroll_offset: 0,
            active_prompt: None,
            working_since: None,
            spinner_frame: 0,
        }
    }

    fn next_id(&mut self) -> BlockId {
        let id = self.next_block_id;
        self.next_block_id += 1;
        id
    }

    /// Adds a finalized user message block.
    pub fn push_user_message(&mut self, text: &str) {
        let id = self.next_id();
        self.viewport_blocks.push(Block::User(UserBlock {
            id,
            ts: Utc::now(),
            content: text.to_string(),
        }));
    }

    /// Ensures an active assistant block exists (creates one if none).
    pub fn ensure_assistant(&mut self) {
        let has_active = self
            .viewport_blocks
            .iter()
            .rev()
            .any(|b| matches!(b, Block::Assistant(a) if a.state == BlockState::Active));
        if !has_active {
            self.start_assistant();
        }
    }

    /// Starts a new active assistant block for streaming.
    pub fn start_assistant(&mut self) {
        let id = self.next_id();
        self.viewport_blocks.push(Block::Assistant(AssistantBlock {
            id,
            ts: Utc::now(),
            content: String::new(),
            state: BlockState::Active,
        }));
    }

    /// Appends a streaming delta to the last active assistant block.
    /// Creates a new assistant block if none is active.
    pub fn append_stream(&mut self, delta: &str) {
        let has_active = self
            .viewport_blocks
            .iter()
            .rev()
            .any(|b| matches!(b, Block::Assistant(a) if a.state == BlockState::Active));

        if !has_active {
            self.start_assistant();
        }

        if let Some(Block::Assistant(ab)) = self
            .viewport_blocks
            .iter_mut()
            .rev()
            .find(|b| matches!(b, Block::Assistant(a) if a.state == BlockState::Active))
        {
            ab.append_delta(delta);
        }

        // Auto-scroll to bottom on new content
        self.scroll_offset = 0;
    }

    /// Finalizes the last active assistant block.
    pub fn finalize_assistant(&mut self) {
        for block in self.viewport_blocks.iter_mut().rev() {
            if let Block::Assistant(ab) = block
                && ab.state == BlockState::Active
            {
                ab.finalize();
                break;
            }
        }
    }

    /// Adds an active tool call block. Collapsed by default.
    pub fn add_tool_call(&mut self, call_id: &str, name: &str, arguments: &str) {
        self.finalize_pending_tools();
        let id = self.next_id();
        self.viewport_blocks.push(Block::ToolCall(ToolCallBlock {
            id,
            ts: Utc::now(),
            call_id: call_id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            result: None,
            is_error: false,
            collapsed: true,
            state: BlockState::Active,
        }));
    }

    /// Sets the result on the tool call block matching `call_id`.
    pub fn set_tool_result(&mut self, call_id: &str, result: &str, is_error: bool) {
        for block in self.viewport_blocks.iter_mut().rev() {
            if let Block::ToolCall(tc) = block
                && tc.call_id == call_id
            {
                tc.set_result(result.to_string(), is_error);
                break;
            }
        }
    }

    /// Adds a finalized tool block from history (collapsed, with result).
    pub fn add_history_tool(&mut self, name: &str, content: &str) {
        let id = self.next_id();
        self.viewport_blocks.push(Block::ToolCall(ToolCallBlock {
            id,
            ts: Utc::now(),
            call_id: String::new(),
            name: name.to_string(),
            arguments: String::new(),
            result: Some(content.to_string()),
            is_error: false,
            collapsed: true,
            state: BlockState::Finalized,
        }));
    }

    /// Finalizes all pending (active) tool call blocks.
    pub fn finalize_pending_tools(&mut self) {
        for block in &mut self.viewport_blocks {
            if let Block::ToolCall(tc) = block
                && tc.state == BlockState::Active
            {
                tc.finalize();
            }
        }
    }

    /// Adds a finalized system message block.
    pub fn add_system_message(&mut self, text: &str) {
        let id = self.next_id();
        self.viewport_blocks.push(Block::System(SystemBlock {
            id,
            ts: Utc::now(),
            content: text.to_string(),
        }));
    }

    /// Drains all finalized blocks from the front of the viewport.
    /// Stops at the first non-finalized block.
    pub fn drain_finalized(&mut self) -> Vec<Block> {
        let first_active = self
            .viewport_blocks
            .iter()
            .position(|b| !b.is_finalized())
            .unwrap_or(self.viewport_blocks.len());

        if first_active == 0 {
            return Vec::new();
        }

        // Adjust selected_block index
        if let Some(sel) = self.selected_block {
            if sel < first_active {
                self.selected_block = None;
            } else {
                self.selected_block = Some(sel - first_active);
            }
        }

        self.viewport_blocks.drain(..first_active).collect()
    }

    /// Toggles collapse on the selected tool call block.
    pub fn toggle_selected_collapse(&mut self) {
        if let Some(idx) = self.selected_block
            && let Some(Block::ToolCall(tc)) = self.viewport_blocks.get_mut(idx)
        {
            tc.toggle_collapse();
        }
    }

    /// Selects the next tool call block in the viewport.
    pub fn select_next_tool(&mut self) {
        let start = self.selected_block.map(|i| i + 1).unwrap_or(0);
        for i in start..self.viewport_blocks.len() {
            if matches!(self.viewport_blocks[i], Block::ToolCall(_)) {
                self.selected_block = Some(i);
                return;
            }
        }
        // Wrap around
        for i in 0..start.min(self.viewport_blocks.len()) {
            if matches!(self.viewport_blocks[i], Block::ToolCall(_)) {
                self.selected_block = Some(i);
                return;
            }
        }
        self.selected_block = None;
    }

    /// Deselects the current block.
    pub fn deselect(&mut self) {
        self.selected_block = None;
    }

    // ── Spinner / working state ─────────────────────────────────────────

    /// Marks the agent as working (starts elapsed timer).
    pub fn start_working(&mut self) {
        if self.working_since.is_none() {
            self.working_since = Some(std::time::Instant::now());
        }
    }

    /// Clears the working state.
    pub fn stop_working(&mut self) {
        self.working_since = None;
        self.spinner_frame = 0;
    }

    /// Advances the spinner animation by one frame.
    pub fn advance_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    // ── Scroll ──────────────────────────────────────────────────────────

    /// Scrolls up by `n` lines. No upper bound here — clamped during render.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    /// Scrolls down by `n` lines (towards bottom).
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    // ── Prompt ──────────────────────────────────────────────────────────

    /// Sets the active prompt for dangerous tool approval.
    pub fn set_prompt(&mut self, token: String, label: String, prompt_type: String) {
        self.active_prompt = Some(ActivePrompt::new(token, label, prompt_type));
    }

    /// Clears the active prompt after a response is sent.
    pub fn clear_prompt(&mut self) {
        self.active_prompt = None;
    }

    /// Returns true when a prompt is active (input should be disabled).
    pub fn has_prompt(&self) -> bool {
        self.active_prompt.is_some()
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_app() {
        let app = App::new();
        assert!(app.viewport_blocks.is_empty());
        assert!(app.input.is_empty());
        assert_eq!(app.connection, ConnectionState::Disconnected);
    }

    #[test]
    fn push_user_and_stream() {
        let mut app = App::new();
        app.push_user_message("hello");
        assert_eq!(app.viewport_blocks.len(), 1);
        assert!(matches!(app.viewport_blocks[0], Block::User(_)));

        app.append_stream("Hi");
        app.append_stream(" there!");
        assert_eq!(app.viewport_blocks.len(), 2);
        if let Block::Assistant(ref ab) = app.viewport_blocks[1] {
            assert_eq!(ab.content, "Hi there!");
            assert_eq!(ab.state, BlockState::Active);
        } else {
            panic!("expected AssistantBlock");
        }

        app.finalize_assistant();
        if let Block::Assistant(ref ab) = app.viewport_blocks[1] {
            assert_eq!(ab.state, BlockState::Finalized);
        }
    }

    #[test]
    fn tool_call_lifecycle() {
        let mut app = App::new();
        app.add_tool_call("tc_1", "file_read", "{}");
        assert_eq!(app.viewport_blocks.len(), 1);
        if let Block::ToolCall(ref tc) = app.viewport_blocks[0] {
            assert!(tc.collapsed);
            assert_eq!(tc.state, BlockState::Active);
        }

        // Adding a new tool call finalizes the previous one
        app.add_tool_call("tc_2", "web_search", "{}");
        if let Block::ToolCall(ref tc) = app.viewport_blocks[0] {
            assert_eq!(tc.state, BlockState::Finalized);
        }
        if let Block::ToolCall(ref tc) = app.viewport_blocks[1] {
            assert_eq!(tc.state, BlockState::Active);
        }
    }

    #[test]
    fn drain_finalized() {
        let mut app = App::new();
        app.push_user_message("hello");
        app.add_system_message("connected");
        app.start_assistant(); // active — should not be drained

        let drained = app.drain_finalized();
        assert_eq!(drained.len(), 2);
        assert_eq!(app.viewport_blocks.len(), 1);
    }

    #[test]
    fn select_tool_navigation() {
        let mut app = App::new();
        app.push_user_message("hello");
        app.add_tool_call("tc_a", "tool_a", "{}");
        app.add_tool_call("tc_b", "tool_b", "{}");

        app.select_next_tool();
        assert_eq!(app.selected_block, Some(1)); // tool_a

        app.select_next_tool();
        assert_eq!(app.selected_block, Some(2)); // tool_b

        app.select_next_tool();
        assert_eq!(app.selected_block, Some(1)); // wrap around

        app.deselect();
        assert_eq!(app.selected_block, None);
    }

    #[test]
    fn toggle_collapse() {
        let mut app = App::new();
        app.add_tool_call("tc_1", "file_read", "{}");
        app.selected_block = Some(0);

        app.toggle_selected_collapse();
        if let Block::ToolCall(ref tc) = app.viewport_blocks[0] {
            assert!(!tc.collapsed);
        }

        app.toggle_selected_collapse();
        if let Block::ToolCall(ref tc) = app.viewport_blocks[0] {
            assert!(tc.collapsed);
        }
    }

    #[test]
    fn input_editing() {
        let mut app = App::new();
        app.input.insert_char('h');
        app.input.insert_char('i');
        assert_eq!(app.input.text, "hi");

        let text = app.input.take_input();
        assert_eq!(text, "hi");
        assert!(app.input.is_empty());
    }
}
