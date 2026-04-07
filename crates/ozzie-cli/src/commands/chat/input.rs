//! Crossterm-based inline input editor for the REPL.
//!
//! Supports multiline editing (Alt+Enter), command history (Up/Down),
//! and standard line-editing shortcuts.

use std::io::{self, Write};

use crossterm::cursor::MoveToColumn;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};

/// Result of reading one input from the user.
pub enum InputResult {
    /// User submitted text (Enter on non-empty buffer).
    Submit(String),
    /// User cancelled (Ctrl+C).
    Cancel,
    /// User wants to quit (Ctrl+D on empty buffer).
    Quit,
}

/// Persistent state across input calls (history).
pub struct InputState {
    history: Vec<String>,
    history_index: Option<usize>,
    history_backup: Option<String>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            history_index: None,
            history_backup: None,
        }
    }

    /// Reads one user input, handling key events in raw mode.
    pub fn read(&mut self, prompt: &str) -> io::Result<InputResult> {
        let mut buffer = String::new();
        let mut cursor = 0usize;
        self.history_index = None;
        self.history_backup = None;

        terminal::enable_raw_mode()?;
        let result = self.read_loop(prompt, &mut buffer, &mut cursor);
        terminal::disable_raw_mode()?;

        // Move to new line after input
        execute!(io::stderr(), MoveToColumn(0))?;

        result
    }

    fn read_loop(
        &mut self,
        prompt: &str,
        buffer: &mut String,
        cursor: &mut usize,
    ) -> io::Result<InputResult> {
        self.render(prompt, buffer, *cursor)?;

        loop {
            let Event::Key(key) = event::read()? else {
                continue;
            };

            // Only handle Press events (not Release/Repeat on some platforms)
            if key.kind != event::KeyEventKind::Press {
                continue;
            }

            match self.handle_key(key, buffer, cursor)? {
                KeyAction::Continue => self.render(prompt, buffer, *cursor)?,
                KeyAction::Submit => {
                    let text = buffer.trim().to_string();
                    eprintln!();
                    if !text.is_empty() {
                        self.history.push(text.clone());
                    }
                    return Ok(InputResult::Submit(text));
                }
                KeyAction::Cancel => {
                    eprintln!("^C");
                    return Ok(InputResult::Cancel);
                }
                KeyAction::Quit => {
                    return Ok(InputResult::Quit);
                }
            }
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        buffer: &mut String,
        cursor: &mut usize,
    ) -> io::Result<KeyAction> {
        match (key.code, key.modifiers) {
            // Submit: Enter (on non-empty, non-continuation buffer)
            (KeyCode::Enter, mods) if !mods.contains(KeyModifiers::ALT) => {
                if buffer.is_empty() {
                    return Ok(KeyAction::Continue);
                }
                // If last line ends with '\', it's a continuation — insert newline
                if buffer.ends_with('\\') {
                    buffer.pop(); // remove the backslash
                    buffer.push('\n');
                    *cursor = buffer.len();
                    return Ok(KeyAction::Continue);
                }
                return Ok(KeyAction::Submit);
            }

            // Newline: Alt+Enter
            (KeyCode::Enter, mods) if mods.contains(KeyModifiers::ALT) => {
                buffer.insert(*cursor, '\n');
                *cursor += '\n'.len_utf8();
            }

            // Cancel: Ctrl+C
            (KeyCode::Char('c'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                return Ok(KeyAction::Cancel);
            }

            // Quit: Ctrl+D on empty buffer
            (KeyCode::Char('d'), mods)
                if mods.contains(KeyModifiers::CONTROL) && buffer.is_empty() =>
            {
                return Ok(KeyAction::Quit);
            }

            // Clear line: Ctrl+U
            (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                buffer.clear();
                *cursor = 0;
            }

            // Beginning of line: Ctrl+A / Home
            (KeyCode::Char('a'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                *cursor = 0;
            }
            (KeyCode::Home, _) => {
                *cursor = 0;
            }

            // End of line: Ctrl+E / End
            (KeyCode::Char('e'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                *cursor = buffer.len();
            }
            (KeyCode::End, _) => {
                *cursor = buffer.len();
            }

            // Delete backward: Backspace
            (KeyCode::Backspace, _) => {
                if *cursor > 0 {
                    let prev = buffer[..*cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    buffer.remove(prev);
                    *cursor = prev;
                }
            }

            // Delete forward: Delete / Ctrl+D (on non-empty)
            (KeyCode::Delete, _)
            | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                if *cursor < buffer.len() {
                    buffer.remove(*cursor);
                }
            }

            // Move left
            (KeyCode::Left, _) => {
                if *cursor > 0 {
                    *cursor = buffer[..*cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }

            // Move right
            (KeyCode::Right, _) => {
                if *cursor < buffer.len() {
                    *cursor += buffer[*cursor..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                }
            }

            // History: Up
            (KeyCode::Up, _) => {
                if self.history.is_empty() {
                    return Ok(KeyAction::Continue);
                }
                if self.history_index.is_none() {
                    self.history_backup = Some(buffer.clone());
                    self.history_index = Some(self.history.len() - 1);
                } else if let Some(idx) = self.history_index
                    && idx > 0
                {
                    self.history_index = Some(idx - 1);
                }
                if let Some(idx) = self.history_index {
                    *buffer = self.history[idx].clone();
                    *cursor = buffer.len();
                }
            }

            // History: Down
            (KeyCode::Down, _) => {
                if let Some(idx) = self.history_index {
                    if idx + 1 < self.history.len() {
                        self.history_index = Some(idx + 1);
                        *buffer = self.history[idx + 1].clone();
                        *cursor = buffer.len();
                    } else {
                        // Restore original input
                        self.history_index = None;
                        *buffer = self.history_backup.take().unwrap_or_default();
                        *cursor = buffer.len();
                    }
                }
            }

            // Regular character
            (KeyCode::Char(c), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                buffer.insert(*cursor, c);
                *cursor += c.len_utf8();
            }

            // Tab: insert spaces
            (KeyCode::Tab, _) => {
                buffer.insert_str(*cursor, "    ");
                *cursor += 4;
            }

            _ => {}
        }

        Ok(KeyAction::Continue)
    }

    fn render(&self, prompt: &str, buffer: &str, _cursor: usize) -> io::Result<()> {
        let mut stderr = io::stderr();

        // Clear current line and print prompt + buffer
        queue!(
            stderr,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
        )?;

        // For multiline, show the first line with prompt, rest indented
        let lines: Vec<&str> = buffer.split('\n').collect();
        if lines.len() <= 1 {
            write!(stderr, "{prompt}{buffer}")?;
        } else {
            let indent = " ".repeat(prompt.len());
            for (i, line) in lines.iter().enumerate() {
                if i == 0 {
                    write!(stderr, "{prompt}{line}")?;
                } else {
                    write!(stderr, "\r\n{indent}{line}")?;
                }
            }
        }

        stderr.flush()
    }
}

enum KeyAction {
    Continue,
    Submit,
    Cancel,
    Quit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_history() {
        let state = InputState::new();
        assert!(state.history.is_empty());
    }

    #[test]
    fn handle_key_char_inserts() {
        let mut state = InputState::new();
        let mut buffer = String::new();
        let mut cursor = 0;
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Continue));
        assert_eq!(buffer, "a");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn handle_key_backspace() {
        let mut state = InputState::new();
        let mut buffer = "ab".to_string();
        let mut cursor = 2;

        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert_eq!(buffer, "a");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn handle_key_ctrl_c_cancels() {
        let mut state = InputState::new();
        let mut buffer = String::new();
        let mut cursor = 0;
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Cancel));
    }

    #[test]
    fn handle_key_ctrl_d_quits_on_empty() {
        let mut state = InputState::new();
        let mut buffer = String::new();
        let mut cursor = 0;
        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Quit));
    }

    #[test]
    fn handle_key_ctrl_d_deletes_on_nonempty() {
        let mut state = InputState::new();
        let mut buffer = "ab".to_string();
        let mut cursor = 0;
        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Continue));
        assert_eq!(buffer, "b");
    }

    #[test]
    fn handle_key_enter_submits_nonempty() {
        let mut state = InputState::new();
        let mut buffer = "hello".to_string();
        let mut cursor = 5;
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Submit));
    }

    #[test]
    fn handle_key_enter_ignores_empty() {
        let mut state = InputState::new();
        let mut buffer = String::new();
        let mut cursor = 0;
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Continue));
    }

    #[test]
    fn handle_key_ctrl_u_clears() {
        let mut state = InputState::new();
        let mut buffer = "hello world".to_string();
        let mut cursor = 5;
        let key = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);

        state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(buffer.is_empty());
        assert_eq!(cursor, 0);
    }

    #[test]
    fn continuation_line() {
        let mut state = InputState::new();
        let mut buffer = "hello\\".to_string();
        let mut cursor = 6;
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);

        let action = state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert!(matches!(action, KeyAction::Continue));
        assert_eq!(buffer, "hello\n");
    }

    #[test]
    fn multibyte_char_insert_and_navigation() {
        let mut state = InputState::new();
        let mut buffer = String::new();
        let mut cursor = 0;

        // Type "pré" — 'é' is 2 bytes in UTF-8
        for c in "pré".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        }
        assert_eq!(buffer, "pré");
        assert_eq!(cursor, 4); // p(1) + r(1) + é(2)

        // Move left over 'é' (should jump 2 bytes)
        let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        state.handle_key(left, &mut buffer, &mut cursor).unwrap();
        assert_eq!(cursor, 2);

        // Move right back over 'é'
        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        state.handle_key(right, &mut buffer, &mut cursor).unwrap();
        assert_eq!(cursor, 4);

        // Backspace removes 'é'
        let bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        state.handle_key(bs, &mut buffer, &mut cursor).unwrap();
        assert_eq!(buffer, "pr");
        assert_eq!(cursor, 2);

        // Insert after backspace still works
        let key = KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE);
        state.handle_key(key, &mut buffer, &mut cursor).unwrap();
        assert_eq!(buffer, "pr!");
        assert_eq!(cursor, 3);
    }
}
