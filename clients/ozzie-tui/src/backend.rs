use crate::app::App;
use crate::block::Block;
use crate::render::RenderContext;

/// Semantic UI events emitted by a backend.
/// The runner maps these to App state mutations.
#[derive(Debug)]
pub enum UiEvent {
    /// User submitted a message.
    SendMessage(String),
    /// Toggle collapse on the selected tool block.
    ToggleCollapse,
    /// Select next tool call block.
    SelectNextTool,
    /// Deselect current block.
    Deselect,
    /// Quit the application.
    Quit,
    /// Re-render tick (no state change).
    Tick,
    // — Scroll events —
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    // — Prompt navigation —
    PromptLeft,
    PromptRight,
    PromptConfirm,
    // — Input events —
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputLeft,
    InputRight,
    InputHome,
    InputEnd,
    InputClearLine,
    InputNewline,
    /// Bracketed paste (may contain newlines).
    Paste(String),
}

/// Trait that each UI platform implements (TUI, Tauri, Web, …).
///
/// The generic event loop in `runner::run_ui` drives the backend:
/// it calls `render` / `flush_finalized` to display state, and
/// `next_event` to receive user actions.
pub trait UiBackend {
    /// One-time setup (e.g. enable raw mode, create terminal).
    fn setup(&mut self) -> anyhow::Result<()>;

    /// Cleanup on exit (e.g. disable raw mode).
    fn teardown(&mut self) -> anyhow::Result<()>;

    /// Render the current app state into the viewport.
    fn render(&mut self, app: &App) -> anyhow::Result<()>;

    /// Push finalized blocks out of the viewport into scrollback / history.
    fn flush_finalized(&mut self, blocks: &[Block], ctx: &RenderContext) -> anyhow::Result<()>;

    /// Wait for the next UI event.
    /// Returns `None` when the event source is exhausted (e.g. terminal closed).
    fn next_event(&mut self) -> impl std::future::Future<Output = Option<UiEvent>> + Send;
}
