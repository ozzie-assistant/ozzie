//! Streaming markdown-to-ANSI renderer for terminal output.
//!
//! Accumulates text deltas, renders complete markdown blocks as ANSI escape
//! sequences. Incomplete trailing content is held back until flushed.

use std::fmt::Write;
use std::io::{self, Write as IoWrite};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

// ANSI escape codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";
const STRIKETHROUGH: &str = "\x1b[9m";
const CYAN: &str = "\x1b[36m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const GRAY: &str = "\x1b[90m";

/// Streaming markdown renderer that buffers deltas and renders complete blocks.
pub struct MarkdownStream {
    buffer: String,
    /// Number of bytes already rendered from the buffer.
    rendered_len: usize,
}

impl MarkdownStream {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            rendered_len: 0,
        }
    }

    /// Appends a delta and renders any newly-complete blocks.
    pub fn push(&mut self, delta: &str) -> io::Result<()> {
        self.buffer.push_str(delta);

        // Find the last safe render point: end of last complete line.
        // We hold back the last incomplete line to avoid partial rendering.
        let safe_end = self.buffer[self.rendered_len..]
            .rfind('\n')
            .map(|pos| self.rendered_len + pos + 1);

        if let Some(end) = safe_end {
            let chunk = &self.buffer[self.rendered_len..end];
            if !chunk.is_empty() {
                let rendered = render_ansi(chunk);
                print!("{rendered}");
                io::stdout().flush()?;
            }
            self.rendered_len = end;
        }

        Ok(())
    }

    /// Flushes all remaining content (call on AssistantMessage).
    pub fn flush(&mut self) -> io::Result<()> {
        if self.rendered_len < self.buffer.len() {
            let remaining = &self.buffer[self.rendered_len..];
            if !remaining.is_empty() {
                let rendered = render_ansi(remaining);
                print!("{rendered}");
                io::stdout().flush()?;
            }
        }
        self.buffer.clear();
        self.rendered_len = 0;
        Ok(())
    }
}

/// Renders a complete markdown string to ANSI-escaped text.
pub fn render_ansi(markdown: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, options);

    let mut output = String::new();
    let mut state = RenderState::default();

    for event in parser {
        render_event(event, &mut state, &mut output);
    }

    output
}

#[derive(Default)]
struct RenderState {
    emphasis: usize,
    strong: usize,
    strikethrough: usize,
    heading_level: Option<u8>,
    in_code_block: bool,
    code_language: String,
    code_buffer: String,
    quote_depth: usize,
    list_stack: Vec<ListKind>,
}

enum ListKind {
    Unordered,
    Ordered(u64),
}

fn render_event(event: Event<'_>, state: &mut RenderState, out: &mut String) {
    match event {
        Event::Start(Tag::Heading { level, .. }) => {
            state.heading_level = Some(level as u8);
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            let color = if level == pulldown_cmark::HeadingLevel::H1 {
                CYAN
            } else {
                BLUE
            };
            let _ = write!(out, "{BOLD}{color}");
        }
        Event::End(TagEnd::Heading(..)) => {
            state.heading_level = None;
            let _ = writeln!(out, "{RESET}");
        }
        Event::Start(Tag::Strong) => {
            state.strong += 1;
            out.push_str(BOLD);
        }
        Event::End(TagEnd::Strong) => {
            state.strong = state.strong.saturating_sub(1);
            out.push_str(RESET);
            reapply_style(state, out);
        }
        Event::Start(Tag::Emphasis) => {
            state.emphasis += 1;
            out.push_str(ITALIC);
        }
        Event::End(TagEnd::Emphasis) => {
            state.emphasis = state.emphasis.saturating_sub(1);
            out.push_str(RESET);
            reapply_style(state, out);
        }
        Event::Start(Tag::Strikethrough) => {
            state.strikethrough += 1;
            out.push_str(STRIKETHROUGH);
        }
        Event::End(TagEnd::Strikethrough) => {
            state.strikethrough = state.strikethrough.saturating_sub(1);
            out.push_str(RESET);
            reapply_style(state, out);
        }
        Event::Start(Tag::CodeBlock(kind)) => {
            state.in_code_block = true;
            state.code_language = match kind {
                CodeBlockKind::Fenced(lang) => lang.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            state.code_buffer.clear();
            let label = if state.code_language.is_empty() {
                "code"
            } else {
                &state.code_language
            };
            let _ = writeln!(out, "{DIM}╭─ {label}{RESET}");
        }
        Event::End(TagEnd::CodeBlock) => {
            // Render buffered code
            for line in state.code_buffer.lines() {
                let _ = writeln!(out, "{GREEN}{line}{RESET}");
            }
            let _ = writeln!(out, "{DIM}╰─{RESET}");
            state.in_code_block = false;
            state.code_buffer.clear();
        }
        Event::Start(Tag::BlockQuote(..)) => {
            state.quote_depth += 1;
        }
        Event::End(TagEnd::BlockQuote(..)) => {
            state.quote_depth = state.quote_depth.saturating_sub(1);
        }
        Event::Start(Tag::List(start)) => {
            state.list_stack.push(match start {
                Some(n) => ListKind::Ordered(n),
                None => ListKind::Unordered,
            });
        }
        Event::End(TagEnd::List(..)) => {
            state.list_stack.pop();
            if state.list_stack.is_empty() {
                out.push('\n');
            }
        }
        Event::Start(Tag::Item) => {
            let depth = state.list_stack.len().saturating_sub(1);
            let indent = "  ".repeat(depth);
            let bullet = match state.list_stack.last_mut() {
                Some(ListKind::Ordered(n)) => {
                    let s = format!("{n}. ");
                    *n += 1;
                    s
                }
                _ => "• ".to_string(),
            };
            let _ = write!(out, "{indent}{bullet}");
        }
        Event::End(TagEnd::Item) if !out.ends_with('\n') => {
            out.push('\n');
        }
        Event::End(TagEnd::Paragraph) => {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        Event::Code(code) => {
            let _ = write!(out, "{YELLOW}`{code}`{RESET}");
            reapply_style(state, out);
        }
        Event::Text(text) => {
            if state.in_code_block {
                state.code_buffer.push_str(&text);
            } else if state.quote_depth > 0 {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    let _ = write!(out, "{GRAY}│ {line}{RESET}");
                }
            } else {
                out.push_str(&text);
            }
        }
        Event::Start(Tag::Link { dest_url, .. }) => {
            let _ = write!(out, "{UNDERLINE}{BLUE}");
            // Store URL for end tag — we just render the text underlined
            // and append the URL after.
            state.code_buffer = dest_url.to_string(); // reuse buffer temporarily
        }
        Event::End(TagEnd::Link) => {
            let url = std::mem::take(&mut state.code_buffer);
            let _ = write!(out, "{RESET} {GRAY}({url}){RESET}");
            reapply_style(state, out);
        }
        Event::SoftBreak | Event::HardBreak => out.push('\n'),
        Event::Rule => {
            let _ = writeln!(out, "{DIM}───{RESET}");
        }
        _ => {}
    }
}

/// Reapplies active inline styles after a RESET.
fn reapply_style(state: &RenderState, out: &mut String) {
    if state.strong > 0 {
        out.push_str(BOLD);
    }
    if state.emphasis > 0 {
        out.push_str(ITALIC);
    }
    if state.strikethrough > 0 {
        out.push_str(STRIKETHROUGH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        let out = render_ansi("hello world");
        assert!(out.contains("hello world"));
    }

    #[test]
    fn bold_text() {
        let out = render_ansi("**bold**");
        assert!(out.contains(BOLD));
        assert!(out.contains("bold"));
        assert!(out.contains(RESET));
    }

    #[test]
    fn italic_text() {
        let out = render_ansi("*italic*");
        assert!(out.contains(ITALIC));
        assert!(out.contains("italic"));
    }

    #[test]
    fn inline_code() {
        let out = render_ansi("use `code` here");
        assert!(out.contains(YELLOW));
        assert!(out.contains("`code`"));
    }

    #[test]
    fn code_block() {
        let out = render_ansi("```rust\nlet x = 1;\n```");
        assert!(out.contains("╭─ rust"));
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("╰─"));
    }

    #[test]
    fn heading() {
        let out = render_ansi("# Title");
        assert!(out.contains(BOLD));
        assert!(out.contains(CYAN));
        assert!(out.contains("Title"));
    }

    #[test]
    fn unordered_list() {
        let out = render_ansi("- one\n- two");
        assert!(out.contains("• one"));
        assert!(out.contains("• two"));
    }

    #[test]
    fn ordered_list() {
        let out = render_ansi("1. first\n2. second");
        assert!(out.contains("1. first"));
        assert!(out.contains("2. second"));
    }

    #[test]
    fn blockquote() {
        let out = render_ansi("> quoted");
        assert!(out.contains("│ quoted"));
    }

    #[test]
    fn streamer_buffers_incomplete_lines() {
        let mut stream = MarkdownStream::new();
        // Push partial line — should not render
        stream.push("hello").unwrap();
        assert_eq!(stream.rendered_len, 0);

        // Push newline — should render
        stream.push(" world\n").unwrap();
        assert!(stream.rendered_len > 0);
    }

    #[test]
    fn streamer_flush() {
        let mut stream = MarkdownStream::new();
        stream.push("trailing content").unwrap();
        assert_eq!(stream.rendered_len, 0);
        stream.flush().unwrap();
        assert_eq!(stream.buffer.len(), 0);
    }

    #[test]
    fn link_rendering() {
        let out = render_ansi("[click](https://example.com)");
        assert!(out.contains("click"));
        assert!(out.contains("https://example.com"));
    }
}
