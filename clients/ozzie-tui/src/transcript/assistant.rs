use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::Shimmer;

use super::cell::HistoryCell;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const LABEL: &str = "Ozzie › ";
const LABEL_BLANK: &str = "        ";

#[derive(Debug)]
pub struct AssistantCell {
    /// Lignes finalisées (ne seront plus modifiées).
    committed: Vec<Line<'static>>,
    /// Texte du delta courant pas encore converti en Line.
    pending: String,
    streaming: bool,
    spinner_frame: usize,
    shimmer: Option<Shimmer>,
}

impl AssistantCell {
    pub fn new_streaming() -> Self {
        Self {
            committed: Vec::new(),
            pending: String::new(),
            streaming: true,
            spinner_frame: 0,
            shimmer: Some(Shimmer::new()),
        }
    }

    /// Ajoute une ligne pré-rendue (provenant du StreamController).
    pub fn push_line(&mut self, line: Line<'static>) {
        self.committed.push(line);
    }

    /// Ajoute un delta de texte brut (pour usage direct sans StreamController).
    #[allow(dead_code)]
    pub fn push_delta(&mut self, delta: &str) {
        self.pending.push_str(delta);
        while let Some(pos) = self.pending.find('\n') {
            let line_text = self.pending[..pos].to_owned();
            self.pending = self.pending[pos + 1..].to_owned();
            self.committed.push(make_content_line(&line_text));
        }
    }

    /// Finalise le streaming : flush le reste dans committed.
    pub fn finalize(&mut self) {
        if !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            self.committed.push(make_content_line(&text));
        }
        self.streaming = false;
        self.shimmer = None;
    }
}

impl HistoryCell for AssistantCell {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let label_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
        let mut lines: Vec<Line<'static>> = Vec::new();

        for (i, line) in self.committed.iter().enumerate() {
            let prefix = if i == 0 {
                Span::styled(LABEL.to_owned(), label_style)
            } else {
                Span::raw(LABEL_BLANK.to_owned())
            };
            let mut spans = vec![prefix];
            spans.extend(line.spans.clone());
            lines.push(Line::from(spans));
        }

        if self.streaming {
            let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
            let shimmer_color = self
                .shimmer
                .as_ref()
                .map(|s| s.color())
                .unwrap_or(Color::DarkGray);
            let prefix = if lines.is_empty() {
                Span::styled(LABEL.to_owned(), label_style)
            } else {
                Span::raw(LABEL_BLANK.to_owned())
            };
            if self.pending.is_empty() {
                lines.push(Line::from(vec![
                    prefix,
                    Span::styled(
                        spinner.to_owned(),
                        Style::default().fg(shimmer_color),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    prefix,
                    Span::styled(
                        self.pending.clone(),
                        Style::default().fg(shimmer_color),
                    ),
                    Span::styled(
                        format!(" {spinner}"),
                        Style::default().fg(shimmer_color),
                    ),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(LABEL.to_owned(), label_style)));
        }

        lines
    }

    fn tick(&mut self) -> bool {
        if self.streaming {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

fn make_content_line(text: &str) -> Line<'static> {
    Line::from(Span::raw(text.to_owned()))
}
