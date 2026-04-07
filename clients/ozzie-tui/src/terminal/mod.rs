mod block_render;
mod input_render;
mod prompt_render;
mod status_render;

use std::io;
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode,
    KeyboardEnhancementFlags, KeyModifiers, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::Widget;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::app::App;
use crate::backend::{UiBackend, UiEvent};
use crate::block::Block;
use crate::render::RenderContext;

/// Maximum viewport height (lines).
/// Kept small: only active (streaming) content lives here.
/// Finalized blocks are flushed to terminal scrollback via `insert_before`.
const MAX_VIEWPORT_HEIGHT: u16 = 12;

/// Ratatui-based terminal backend using `Viewport::Inline`.
pub struct TerminalBackend {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    reader: EventStream,
    tick: tokio::time::Interval,
    /// Whether the kitty keyboard protocol is active (enables Shift+Enter detection).
    enhanced_keys: bool,
}

impl TerminalBackend {
    pub fn new() -> Self {
        Self {
            terminal: None,
            reader: EventStream::new(),
            tick: tokio::time::interval(Duration::from_millis(50)),
            enhanced_keys: false,
        }
    }
}

impl Default for TerminalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl UiBackend for TerminalBackend {
    fn setup(&mut self) -> anyhow::Result<()> {
        // Panic hook to restore terminal
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = crossterm::execute!(
                io::stdout(),
                DisableBracketedPaste,
                PopKeyboardEnhancementFlags
            );
            original_hook(info);
        }));

        enable_raw_mode()?;
        crossterm::execute!(io::stdout(), EnableBracketedPaste)?;

        // Enable kitty keyboard protocol if terminal supports it (Shift+Enter detection)
        if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
            crossterm::execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            self.enhanced_keys = true;
        }

        let backend = CrosstermBackend::new(io::stdout());

        // Dynamic viewport height: use terminal height capped at MAX_VIEWPORT_HEIGHT
        let term_height = crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24);
        let viewport_height = term_height.min(MAX_VIEWPORT_HEIGHT);

        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(viewport_height),
            },
        )?;
        self.terminal = Some(terminal);
        Ok(())
    }

    fn teardown(&mut self) -> anyhow::Result<()> {
        if self.enhanced_keys {
            let _ = crossterm::execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        crossterm::execute!(io::stdout(), DisableBracketedPaste)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn render(&mut self, app: &App) -> anyhow::Result<()> {
        let terminal = self.terminal.as_mut().expect("setup() not called");
        terminal.draw(|frame| {
            render_viewport(frame, app);
        })?;
        Ok(())
    }

    fn flush_finalized(&mut self, blocks: &[Block], ctx: &RenderContext) -> anyhow::Result<()> {
        let terminal = self.terminal.as_mut().expect("setup() not called");
        let width = terminal.size()?.width;
        for block in blocks {
            let lines = render_scrollback_block(block, width, ctx);
            let n = lines.len() as u16;
            if n > 0 {
                terminal.insert_before(n, |buf| {
                    for (i, line) in lines.iter().enumerate() {
                        let area = Rect::new(buf.area.x, buf.area.y + i as u16, buf.area.width, 1);
                        Paragraph::new(line.clone()).render(area, buf);
                    }
                })?;
            }
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Option<UiEvent> {
        tokio::select! {
            _ = self.tick.tick() => Some(UiEvent::Tick),
            event = self.reader.next() => {
                match event? {
                    Ok(Event::Key(key)) => translate_key(key.code, key.modifiers),
                    Ok(Event::Paste(text)) => Some(UiEvent::Paste(text)),
                    _ => Some(UiEvent::Tick),
                }
            }
        }
    }
}

fn translate_key(code: KeyCode, modifiers: KeyModifiers) -> Option<UiEvent> {
    match (code, modifiers) {
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => Some(UiEvent::Quit),
        (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => Some(UiEvent::InputHome),
        (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => Some(UiEvent::InputEnd),
        (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
            Some(UiEvent::InputClearLine)
        }
        // Ctrl+J: insert newline (fallback for terminals without Shift+Enter)
        (KeyCode::Char('j'), m) if m.contains(KeyModifiers::CONTROL) => {
            Some(UiEvent::InputNewline)
        }
        // Shift+Enter: insert newline (kitty keyboard protocol)
        (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) => Some(UiEvent::InputNewline),
        // Scroll
        (KeyCode::Up, _) => Some(UiEvent::ScrollUp),
        (KeyCode::Down, _) => Some(UiEvent::ScrollDown),
        (KeyCode::PageUp, _) => Some(UiEvent::PageUp),
        (KeyCode::PageDown, _) => Some(UiEvent::PageDown),
        // Prompt navigation (Left/Right double as prompt nav when prompt is active)
        (KeyCode::Left, _) => Some(UiEvent::PromptLeft),
        (KeyCode::Right, _) => Some(UiEvent::PromptRight),
        (KeyCode::Tab, _) => Some(UiEvent::SelectNextTool),
        (KeyCode::Esc, _) => Some(UiEvent::Deselect),
        (KeyCode::Backspace, _) => Some(UiEvent::InputBackspace),
        (KeyCode::Delete, _) => Some(UiEvent::InputDelete),
        (KeyCode::Home, _) => Some(UiEvent::InputHome),
        (KeyCode::End, _) => Some(UiEvent::InputEnd),
        (KeyCode::Char(c), _) => Some(UiEvent::InputChar(c)),
        // Plain Enter: submit message
        (KeyCode::Enter, _) => Some(UiEvent::SendMessage(String::new())),
        _ => None,
    }
}

// ── Viewport rendering ──────────────────────────────────────────────────────

fn render_viewport(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();

    // Input height (dynamic, 1-8 lines)
    let input_h = if app.has_prompt() {
        0u16
    } else {
        input_render::input_height(&app.input)
    };
    let bottom_h: u16 = if app.has_prompt() { 2 } else { 1 };

    // Layout: [content] [input] [status]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),          // content (active blocks only)
            Constraint::Length(input_h),  // input
            Constraint::Length(bottom_h), // status + optional prompt
        ])
        .split(area);

    let content_area = chunks[0];
    let input_area = chunks[1];

    // Render blocks, grouping consecutive tool calls into runs.
    let mut y = content_area.y;
    let max_y = content_area.y + content_area.height;
    let blocks = &app.viewport_blocks;
    let mut i = 0;
    while i < blocks.len() && y < max_y {
        if let Block::ToolCall(_) = &blocks[i] {
            // Collect consecutive tool call blocks
            let run_start = i;
            while i < blocks.len() && matches!(&blocks[i], Block::ToolCall(_)) {
                i += 1;
            }
            let tool_refs: Vec<&crate::block::ToolCallBlock> = blocks[run_start..i]
                .iter()
                .filter_map(|b| match b {
                    Block::ToolCall(tc) => Some(tc),
                    _ => None,
                })
                .collect();
            let sel_in_run = app.selected_block.and_then(|s| {
                if s >= run_start && s < i { Some(s - run_start) } else { None }
            });
            let lines = block_render::render_tool_run(&tool_refs, sel_in_run);
            for line in lines {
                if y >= max_y { break; }
                frame.render_widget(
                    Paragraph::new(line),
                    Rect::new(content_area.x, y, content_area.width, 1),
                );
                y += 1;
            }
        } else {
            let selected = app.selected_block == Some(i);
            let lines = block_render::render_block(&blocks[i], selected, &app.render_ctx);
            for line in lines {
                if y >= max_y { break; }
                frame.render_widget(
                    Paragraph::new(line),
                    Rect::new(content_area.x, y, content_area.width, 1),
                );
                y += 1;
            }
            i += 1;
        }
    }

    // Input
    if !app.has_prompt() && input_area.height > 0 {
        input_render::render_input(frame, &app.input, input_area);
    }

    // Status bar + optional prompt
    if app.has_prompt() {
        let bottom_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // prompt bar
                Constraint::Length(1), // status bar
            ])
            .split(chunks[2]);

        if let Some(ref prompt) = app.active_prompt {
            prompt_render::render_prompt(frame, prompt, bottom_chunks[0]);
        }
        status_render::render_status(
            frame,
            app.connection,
            app.session_id.as_deref(),
            &app.status,
            app.working_since,
            app.spinner_frame,
            bottom_chunks[1],
        );
    } else {
        status_render::render_status(
            frame,
            app.connection,
            app.session_id.as_deref(),
            &app.status,
            app.working_since,
            app.spinner_frame,
            chunks[2],
        );
    }
}

fn render_scrollback_block(block: &Block, _width: u16, ctx: &RenderContext) -> Vec<Line<'static>> {
    block_render::render_block(block, false, ctx)
}
