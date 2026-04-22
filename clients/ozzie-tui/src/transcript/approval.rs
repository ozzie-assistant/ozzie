use ozzie_types::PromptOption;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::cell::HistoryCell;

#[derive(Debug)]
pub struct ApprovalCell {
    #[allow(dead_code)]
    pub token: String,
    pub label: String,
    pub options: Vec<PromptOption>,
    /// Index de l'option sélectionnée par l'overlay, `None` si en attente.
    pub decision: Option<String>,
}

impl ApprovalCell {
    pub fn new(
        token: impl Into<String>,
        label: impl Into<String>,
        options: Vec<PromptOption>,
    ) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            options,
            decision: None,
        }
    }
}

impl HistoryCell for ApprovalCell {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(
                " ⚠ Approbation requise : ".to_owned(),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(self.label.clone()),
        ]));

        if let Some(ref d) = self.decision {
            lines.push(Line::from(vec![
                Span::raw("   → "),
                Span::styled(d.clone(), Style::default().fg(Color::Green)),
            ]));
        } else {
            for opt in &self.options {
                lines.push(Line::from(vec![
                    Span::raw("   • "),
                    Span::styled(
                        opt.label.clone(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        lines
    }
}
