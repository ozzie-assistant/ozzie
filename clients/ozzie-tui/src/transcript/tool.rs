use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::cell::HistoryCell;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug)]
pub struct ToolCell {
    pub call_id: String,
    pub tool: String,
    args: String,
    result: Option<ToolResult>,
    spinner_frame: usize,
}

#[derive(Debug)]
struct ToolResult {
    text: String,
    is_error: bool,
}

impl ToolCell {
    pub fn new(call_id: impl Into<String>, tool: impl Into<String>, args: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            tool: tool.into(),
            args: args.into(),
            result: None,
            spinner_frame: 0,
        }
    }

    pub fn set_result(&mut self, text: impl Into<String>, is_error: bool) {
        self.result = Some(ToolResult { text: text.into(), is_error });
    }
}

impl HistoryCell for ToolCell {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let running = self.result.is_none();
        let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];

        let tool_label = if running {
            Span::styled(
                format!(" {spinner} {} ", self.tool),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!(" ⚙ {} ", self.tool),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
            )
        };

        let mut lines = vec![Line::from(vec![tool_label])];

        if !self.args.is_empty() && self.args != "{}" {
            let truncated = truncate_str(&self.args, 120);
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(truncated, Style::default().fg(Color::DarkGray)),
            ]));
        }

        if let Some(ref r) = self.result {
            let color = if r.is_error { Color::Red } else { Color::Green };
            let icon = if r.is_error { "✗" } else { "✓" };
            let truncated = truncate_str(&r.text, 200);
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(format!("{icon} "), Style::default().fg(color)),
                Span::styled(truncated, Style::default().fg(Color::DarkGray)),
            ]));
        }

        lines
    }

    fn tick(&mut self) -> bool {
        if self.result.is_none() {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let boundary = s
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0);
    format!("{}…", &s[..boundary])
}
