use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
// KeyEvent importé pour la signature de handle_key.
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

/// Éditeur de texte multi-ligne avec bindings Emacs de base.
///
/// Gère : insertion, backspace/delete, navigation curseur, kill-buffer (Ctrl+K/Y),
/// newline (Shift+Enter ou Alt+Enter) et soumission (Enter).
pub struct TextArea {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    kill_buffer: String,
}

pub enum TextAreaAction {
    /// L'utilisateur a soumis le texte.
    Submit(String),
    /// Touche gérée normalement, redraw requis.
    Handled,
    /// Touche non gérée par le TextArea — renvoyée au parent.
    Unhandled,
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            kill_buffer: String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_empty())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.lines().map(|l| l.to_owned()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TextAreaAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            // Soumission
            KeyCode::Enter if !shift && !alt => {
                if self.is_empty() {
                    return TextAreaAction::Handled;
                }
                let text = self.text();
                self.clear();
                return TextAreaAction::Submit(text);
            }
            // Newline
            KeyCode::Enter if shift || alt => {
                self.insert_newline();
            }
            // Navigation
            KeyCode::Left | KeyCode::Char('b') if ctrl => self.move_left(),
            KeyCode::Right | KeyCode::Char('f') if ctrl => self.move_right(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => {
                if self.cursor_row == 0 {
                    return TextAreaAction::Unhandled;
                }
                self.move_up();
            }
            KeyCode::Down => {
                if self.cursor_row == self.lines.len() - 1 {
                    return TextAreaAction::Unhandled;
                }
                self.move_down();
            }
            KeyCode::Home | KeyCode::Char('a') if ctrl => {
                self.cursor_col = 0;
            }
            KeyCode::End | KeyCode::Char('e') if ctrl => {
                self.cursor_col = self.current_line().len();
            }
            // Suppression
            KeyCode::Backspace => self.delete_back(),
            KeyCode::Delete => self.delete_forward(),
            // Kill buffer (Emacs)
            KeyCode::Char('k') if ctrl => self.kill_to_eol(),
            KeyCode::Char('y') if ctrl => self.yank(),
            KeyCode::Char('u') if ctrl => {
                // Kill from beginning of line
                let col = self.cursor_col;
                let line = self.lines[self.cursor_row].clone();
                self.kill_buffer = line[..col].to_owned();
                self.lines[self.cursor_row] = line[col..].to_owned();
                self.cursor_col = 0;
            }
            // Insertion caractère
            KeyCode::Char(c) if !ctrl && !alt => {
                self.insert_char(c);
            }
            _ => return TextAreaAction::Unhandled,
        }
        TextAreaAction::Handled
    }

    /// Rendu du widget dans le block fourni.
    pub fn render_widget<'a>(&self, block: Block<'a>) -> Paragraph<'a> {
        let cursor_style = Style::default()
            .bg(Color::White)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD);

        let lines: Vec<Line> = self
            .lines
            .iter()
            .enumerate()
            .map(|(row, line)| {
                if row == self.cursor_row {
                    // Insère le curseur visuel
                    let before = &line[..self.cursor_col.min(line.len())];
                    let at = line
                        .chars()
                        .nth(self.cursor_col)
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| " ".to_string());
                    let after = if self.cursor_col < line.len() {
                        &line[self.cursor_col + at.len().min(line.len() - self.cursor_col)..]
                    } else {
                        ""
                    };
                    Line::from(vec![
                        Span::raw(before.to_owned()),
                        Span::styled(at, cursor_style),
                        Span::raw(after.to_owned()),
                    ])
                } else {
                    Line::from(Span::raw(line.clone()))
                }
            })
            .collect();

        Paragraph::new(lines).block(block)
    }

    // ── Primitives de mutation ─────────────────────────────────────────────

    fn current_line(&self) -> &str {
        &self.lines[self.cursor_row]
    }

    fn insert_char(&mut self, c: char) {
        let col = self.cursor_col;
        self.lines[self.cursor_row].insert(col, c);
        self.cursor_col += c.len_utf8();
    }

    fn insert_newline(&mut self) {
        let col = self.cursor_col;
        let tail = self.lines[self.cursor_row][col..].to_owned();
        self.lines[self.cursor_row].truncate(col);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, tail);
        self.cursor_col = 0;
    }

    fn delete_back(&mut self) {
        if self.cursor_col > 0 {
            let col = self.cursor_col;
            let c = self.lines[self.cursor_row]
                .chars()
                .nth(self.char_index_before(col))
                .unwrap_or(' ');
            let byte_pos = col - c.len_utf8();
            self.lines[self.cursor_row].remove(byte_pos);
            self.cursor_col = byte_pos;
        } else if self.cursor_row > 0 {
            let tail = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&tail);
        }
    }

    fn delete_forward(&mut self) {
        let col = self.cursor_col;
        let len = self.lines[self.cursor_row].len();
        if col < len {
            self.lines[self.cursor_row].remove(col);
        } else if self.cursor_row < self.lines.len() - 1 {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    fn kill_to_eol(&mut self) {
        let col = self.cursor_col;
        let line = &self.lines[self.cursor_row];
        if col < line.len() {
            self.kill_buffer = line[col..].to_owned();
            self.lines[self.cursor_row].truncate(col);
        } else if self.cursor_row < self.lines.len() - 1 {
            self.kill_buffer = "\n".to_owned();
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    fn yank(&mut self) {
        let yank = self.kill_buffer.clone();
        for c in yank.chars() {
            if c == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(c);
            }
        }
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            let c = self.lines[self.cursor_row]
                .chars()
                .nth(self.char_index_before(self.cursor_col))
                .unwrap_or(' ');
            self.cursor_col -= c.len_utf8();
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
        }
    }

    fn move_right(&mut self) {
        let len = self.lines[self.cursor_row].len();
        if self.cursor_col < len {
            let c = self.lines[self.cursor_row][self.cursor_col..]
                .chars()
                .next()
                .unwrap_or(' ');
            self.cursor_col += c.len_utf8();
        } else if self.cursor_row < self.lines.len() - 1 {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn move_up(&mut self) {
        self.cursor_row = self.cursor_row.saturating_sub(1);
        self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
    }

    fn move_down(&mut self) {
        self.cursor_row = (self.cursor_row + 1).min(self.lines.len() - 1);
        self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
    }

    fn char_index_before(&self, byte_pos: usize) -> usize {
        let line = &self.lines[self.cursor_row];
        line[..byte_pos].chars().count().saturating_sub(1)
    }
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}
