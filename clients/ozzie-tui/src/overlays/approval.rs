use crossterm::event::{KeyCode, KeyEvent};
use ozzie_types::PromptOption;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

use super::{Overlay, ViewAction};

/// Overlay d'approbation pour les dangerous tools.
///
/// Affiche le label + les options et laisse l'utilisateur choisir avec ↑↓ + Enter.
/// Escape → deny (première option avec value "deny" ou la dernière).
#[derive(Debug)]
pub struct ApprovalOverlay {
    token: String,
    label: String,
    options: Vec<PromptOption>,
    selected: usize,
}

impl ApprovalOverlay {
    pub fn new(token: impl Into<String>, label: impl Into<String>, options: Vec<PromptOption>) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            options,
            selected: 0,
        }
    }

    fn selected_value(&self) -> Option<String> {
        self.options.get(self.selected).map(|o| o.value.clone())
    }

    fn deny_value(&self) -> String {
        self.options
            .iter()
            .find(|o| o.value.contains("deny") || o.value.contains("reject") || o.value.contains("no"))
            .map(|o| o.value.clone())
            .unwrap_or_else(|| {
                self.options.last().map(|o| o.value.clone()).unwrap_or_default()
            })
    }
}

impl Overlay for ApprovalOverlay {
    fn token(&self) -> &str {
        &self.token
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                ViewAction::Consumed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.options.len().saturating_sub(1));
                ViewAction::Consumed
            }
            KeyCode::Enter => {
                ViewAction::Done(self.selected_value())
            }
            KeyCode::Esc => {
                ViewAction::Done(Some(self.deny_value()))
            }
            _ => ViewAction::Consumed,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Zone flottante centrée horizontalement, en bas de l'aire disponible.
        let popup_height = (self.options.len() as u16 + 4).min(area.height);
        let popup_width = (area.width * 3 / 4).max(40).min(area.width);
        let popup_x = (area.width - popup_width) / 2;
        let popup_y = area.height.saturating_sub(popup_height);
        let popup_area = Rect::new(
            area.x + popup_x,
            area.y + popup_y,
            popup_width,
            popup_height,
        );

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" ⚠ Approbation requise ")
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let option_height = self.options.len() as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(option_height),
            ])
            .split(inner);

        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(self.label.clone()))),
            chunks[0],
        );

        let option_lines: Vec<Line> = self
            .options
            .iter()
            .enumerate()
            .map(|(i, opt)| {
                let selected = i == self.selected;
                let prefix = if selected { "▶ " } else { "  " };
                let style = if selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!("{prefix}{}", opt.label), style))
            })
            .collect();

        frame.render_widget(Paragraph::new(option_lines), chunks[1]);
    }
}
