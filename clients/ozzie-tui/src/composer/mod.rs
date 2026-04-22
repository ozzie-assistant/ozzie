pub mod textarea;

use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use textarea::{TextArea, TextAreaAction};

/// Zone de saisie : éditeur multi-ligne + historique + footer d'aide.
pub struct ComposerWidget {
    textarea: TextArea,
    history: Vec<String>,
    history_idx: Option<usize>,
    /// Snapshot du textarea au moment où la navigation historique a commencé.
    saved_draft: Option<String>,
    agent_running: bool,
}

pub enum ComposerOutput {
    Submit(String),
    Handled,
}

impl ComposerWidget {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            history: Vec::new(),
            history_idx: None,
            saved_draft: None,
            agent_running: false,
        }
    }

    pub fn set_agent_running(&mut self, running: bool) {
        self.agent_running = running;
    }

    /// Hauteur souhaitée pour le layout (bordures + lignes + footer), max 12.
    pub fn desired_height(&self) -> u16 {
        let content = self.textarea.line_count().clamp(1, 10) as u16;
        2 + content + 1 // bordure top + lignes + bordure bottom + footer
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerOutput> {
        use crossterm::event::KeyCode;

        // Navigation dans l'historique avec ↑ / ↓ quand le textarea le laisse passer.
        if key.code == KeyCode::Up {
            self.history_prev();
            return Some(ComposerOutput::Handled);
        }
        if key.code == KeyCode::Down {
            self.history_next();
            return Some(ComposerOutput::Handled);
        }

        match self.textarea.handle_key(key) {
            TextAreaAction::Submit(text) => {
                if !text.is_empty() {
                    self.commit_to_history(&text);
                    self.history_idx = None;
                    self.saved_draft = None;
                }
                Some(ComposerOutput::Submit(text))
            }
            TextAreaAction::Handled => Some(ComposerOutput::Handled),
            TextAreaAction::Unhandled => None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let help = self.footer_hint();
        let footer_height: u16 = 1;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(footer_height),
            ])
            .split(area);

        let editor_block = Block::bordered().title(if self.agent_running {
            " Agent en cours — ↑↓ scroll · Ctrl+C interrompre "
        } else {
            " Message "
        });
        frame.render_widget(self.textarea.render_widget(editor_block), chunks[0]);

        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                help,
                Style::default().fg(Color::DarkGray),
            )])),
            chunks[1],
        );
    }

    // ── Historique ──────────────────────────────────────────────────────────

    fn commit_to_history(&mut self, text: &str) {
        // Évite les doublons consécutifs.
        if self.history.last().map(|s| s.as_str()) != Some(text) {
            self.history.push(text.to_owned());
        }
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_idx.is_none() {
            self.saved_draft = Some(self.textarea.text());
        }
        let next = match self.history_idx {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(next);
        self.textarea.set_text(&self.history[next].clone());
    }

    fn history_next(&mut self) {
        let Some(idx) = self.history_idx else { return };
        if idx + 1 >= self.history.len() {
            self.history_idx = None;
            let draft = self.saved_draft.take().unwrap_or_default();
            self.textarea.set_text(&draft);
        } else {
            let next = idx + 1;
            self.history_idx = Some(next);
            self.textarea.set_text(&self.history[next].clone());
        }
    }

    fn footer_hint(&self) -> String {
        if self.agent_running {
            "Ctrl+C interrompre".to_owned()
        } else {
            "[Enter] Envoyer · [Shift+↵] Newline · [↑↓] Historique · [Ctrl+Q] Quitter"
                .to_owned()
        }
    }
}

impl Default for ComposerWidget {
    fn default() -> Self {
        Self::new()
    }
}
