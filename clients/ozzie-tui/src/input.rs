/// Maximum visible lines in the input area before scrolling.
pub const MAX_INPUT_LINES: usize = 8;

/// Text input state with multiline cursor management.
///
/// Text is stored as a single `String` with `\n` for line breaks.
/// Cursor is a byte offset into the string.
#[derive(Debug, Clone)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    /// First visible line when input exceeds MAX_INPUT_LINES.
    pub scroll: usize,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            scroll: 0,
        }
    }

    // ── Character editing ───────────────────────────────────────────────

    /// Inserts a character at cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.ensure_cursor_visible();
    }

    /// Inserts a string at cursor position (used for paste).
    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.ensure_cursor_visible();
    }

    /// Inserts a newline at cursor position.
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Deletes the character before cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.text[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.remove(prev);
            self.cursor = prev;
            self.ensure_cursor_visible();
        }
    }

    /// Deletes the character at cursor.
    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    // ── Horizontal movement ─────────────────────────────────────────────

    /// Moves cursor left by one character.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.ensure_cursor_visible();
        }
    }

    /// Moves cursor right by one character.
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
            self.ensure_cursor_visible();
        }
    }

    /// Moves cursor to start of current line.
    pub fn home(&mut self) {
        let (line_start, _) = self.current_line_bounds();
        self.cursor = line_start;
    }

    /// Moves cursor to end of current line.
    pub fn end(&mut self) {
        let (_, line_end) = self.current_line_bounds();
        self.cursor = line_end;
    }

    // ── Vertical movement ───────────────────────────────────────────────

    /// Moves cursor up one line. Returns `true` if moved, `false` if already on first line.
    pub fn move_up(&mut self) -> bool {
        let (row, col) = self.row_col();
        if row == 0 {
            return false;
        }
        self.move_to_row_col(row - 1, col);
        self.ensure_cursor_visible();
        true
    }

    /// Moves cursor down one line. Returns `true` if moved, `false` if already on last line.
    pub fn move_down(&mut self) -> bool {
        let (row, col) = self.row_col();
        if row >= self.line_count() - 1 {
            return false;
        }
        self.move_to_row_col(row + 1, col);
        self.ensure_cursor_visible();
        true
    }

    /// Returns `true` when cursor is on the first line (Up should scroll instead).
    pub fn cursor_on_first_line(&self) -> bool {
        self.row_col().0 == 0
    }

    /// Returns `true` when cursor is on the last line (Down should scroll instead).
    pub fn cursor_on_last_line(&self) -> bool {
        self.row_col().0 >= self.line_count() - 1
    }

    // ── Queries ─────────────────────────────────────────────────────────

    /// Takes the current input text and resets state.
    pub fn take_input(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.scroll = 0;
        text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns true if the input contains multiple lines.
    pub fn is_multiline(&self) -> bool {
        self.text.contains('\n')
    }

    /// Clears all text (Ctrl+U).
    pub fn clear_line(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Number of logical lines in the input.
    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }

    /// Returns the visible line count (capped at MAX_INPUT_LINES).
    pub fn visible_line_count(&self) -> usize {
        self.line_count().min(MAX_INPUT_LINES)
    }

    /// Returns (row, col) of the cursor (0-based, col is char count not bytes).
    pub fn row_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let row = before.matches('\n').count();
        let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = before[last_newline..].chars().count();
        (row, col)
    }

    /// Returns an iterator over (line_text, start_byte_offset) for each line.
    pub fn lines_with_offsets(&self) -> Vec<(&str, usize)> {
        let mut result = Vec::new();
        let mut offset = 0;
        for line in self.text.split('\n') {
            result.push((line, offset));
            offset += line.len() + 1; // +1 for \n
        }
        // Handle empty text
        if result.is_empty() {
            result.push(("", 0));
        }
        result
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Returns the byte range (start, end) of the current line the cursor is on.
    fn current_line_bounds(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        (line_start, line_end)
    }

    /// Moves cursor to (row, col), clamping col to the line length.
    fn move_to_row_col(&mut self, target_row: usize, target_col: usize) {
        let lines = self.lines_with_offsets();
        let row = target_row.min(lines.len() - 1);
        let (line_text, line_offset) = lines[row];
        let char_count = line_text.chars().count();
        let clamped_col = target_col.min(char_count);
        // Convert char col to byte offset within line
        let byte_col = line_text
            .char_indices()
            .nth(clamped_col)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());
        self.cursor = line_offset + byte_col;
    }

    /// Ensures the cursor row is within the visible scroll window.
    fn ensure_cursor_visible(&mut self) {
        let (row, _) = self.row_col();
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + MAX_INPUT_LINES {
            self.scroll = row + 1 - MAX_INPUT_LINES;
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_backspace() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.text, "hi");
        assert_eq!(input.cursor, 2);

        input.backspace();
        assert_eq!(input.text, "h");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn cursor_movement() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');

        input.move_left();
        assert_eq!(input.cursor, 2);
        input.move_left();
        assert_eq!(input.cursor, 1);
        input.move_right();
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn home_end() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');

        input.home();
        assert_eq!(input.cursor, 0);
        input.end();
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn take_input() {
        let mut input = InputState::new();
        input.insert_char('t');
        input.insert_char('e');
        input.insert_char('s');
        input.insert_char('t');

        let text = input.take_input();
        assert_eq!(text, "test");
        assert!(input.is_empty());
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn delete_at_cursor() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.home();
        input.delete();
        assert_eq!(input.text, "bc");
    }

    #[test]
    fn clear_line() {
        let mut input = InputState::new();
        input.insert_char('x');
        input.insert_char('y');
        input.clear_line();
        assert!(input.is_empty());
        assert_eq!(input.cursor, 0);
    }

    // ── Multiline tests ─────────────────────────────────────────────────

    #[test]
    fn insert_newline() {
        let mut input = InputState::new();
        input.insert_str("hello");
        input.insert_newline();
        input.insert_str("world");
        assert_eq!(input.text, "hello\nworld");
        assert_eq!(input.line_count(), 2);
        assert!(input.is_multiline());
    }

    #[test]
    fn row_col() {
        let mut input = InputState::new();
        input.insert_str("abc\ndef\nghi");
        // Cursor at end: row 2, col 3
        assert_eq!(input.row_col(), (2, 3));

        // Move to start of line 2
        input.home();
        assert_eq!(input.row_col(), (2, 0));
    }

    #[test]
    fn move_up_down() {
        let mut input = InputState::new();
        input.insert_str("abc\ndef\nghi");
        // At (2, 3), move up → (1, 3)
        assert!(input.move_up());
        assert_eq!(input.row_col(), (1, 3));

        // Up again → (0, 3)
        assert!(input.move_up());
        assert_eq!(input.row_col(), (0, 3));

        // Can't go up further
        assert!(!input.move_up());
        assert_eq!(input.row_col(), (0, 3));

        // Down to (1, 3)
        assert!(input.move_down());
        assert_eq!(input.row_col(), (1, 3));
    }

    #[test]
    fn move_up_clamps_col() {
        let mut input = InputState::new();
        input.insert_str("abcdef\nhi");
        // At (1, 2), move up → (0, 2) — col clamped to shorter line? No, line 0 is longer
        assert!(input.move_up());
        assert_eq!(input.row_col(), (0, 2));

        // Now test the other direction: long col → short line
        let mut input2 = InputState::new();
        input2.insert_str("ab\ncdefgh");
        // At (1, 6), move up → (0, 2) — clamped
        assert!(input2.move_up());
        assert_eq!(input2.row_col(), (0, 2));
    }

    #[test]
    fn home_end_multiline() {
        let mut input = InputState::new();
        input.insert_str("abc\ndef");
        // Cursor at end of "def"
        input.home();
        assert_eq!(input.row_col(), (1, 0));
        assert_eq!(input.cursor, 4); // byte offset of 'd'

        input.end();
        assert_eq!(input.row_col(), (1, 3));
        assert_eq!(input.cursor, 7);
    }

    #[test]
    fn insert_str_paste() {
        let mut input = InputState::new();
        input.insert_str("line1\nline2\nline3");
        assert_eq!(input.line_count(), 3);
        assert_eq!(input.row_col(), (2, 5));
    }

    #[test]
    fn cursor_on_first_last_line() {
        let mut input = InputState::new();
        input.insert_str("abc\ndef\nghi");
        assert!(!input.cursor_on_first_line());
        assert!(input.cursor_on_last_line());

        input.move_up();
        assert!(!input.cursor_on_first_line());
        assert!(!input.cursor_on_last_line());

        input.move_up();
        assert!(input.cursor_on_first_line());
        assert!(!input.cursor_on_last_line());
    }

    #[test]
    fn scroll_adjusts_on_many_lines() {
        let mut input = InputState::new();
        for i in 0..20 {
            if i > 0 {
                input.insert_newline();
            }
            input.insert_str(&format!("line {i}"));
        }
        // Cursor on last line, scroll should show last MAX_INPUT_LINES
        assert!(input.scroll > 0);
        assert!(input.row_col().0 >= input.scroll);
        assert!(input.row_col().0 < input.scroll + MAX_INPUT_LINES);
    }

    #[test]
    fn lines_with_offsets() {
        let mut input = InputState::new();
        input.insert_str("abc\ndef");
        let lines = input.lines_with_offsets();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], ("abc", 0));
        assert_eq!(lines[1], ("def", 4));
    }
}
