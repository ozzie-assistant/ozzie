use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::cell::HistoryCell;

#[derive(Debug)]
pub struct UserCell {
    text: String,
}

impl UserCell {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

impl HistoryCell for UserCell {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let label = Span::styled(
            "You › ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );

        let mut lines = Vec::new();
        let content_width = (width as usize).saturating_sub(6);

        for (i, src_line) in self.text.lines().enumerate() {
            let prefix = if i == 0 {
                label.clone()
            } else {
                Span::raw("      ")
            };

            if src_line.is_empty() {
                lines.push(Line::from(vec![prefix]));
                continue;
            }

            let chunks = wrap_str(src_line, content_width.max(1));
            for (j, chunk) in chunks.into_iter().enumerate() {
                let pfx = if i == 0 && j == 0 {
                    label.clone()
                } else {
                    Span::raw("      ")
                };
                lines.push(Line::from(vec![pfx, Span::raw(chunk)]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![label, Span::raw("")]));
        }

        lines
    }
}

/// Découpe `s` en tranches de `width` caractères maximum.
fn wrap_str(s: &str, width: usize) -> Vec<String> {
    if s.len() <= width {
        return vec![s.to_owned()];
    }
    s.chars()
        .collect::<Vec<_>>()
        .chunks(width)
        .map(|c| c.iter().collect())
        .collect()
}
