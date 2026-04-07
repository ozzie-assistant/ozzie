use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::input::InputState;

/// Renders the multiline input area.
///
/// First line gets the `❯` prompt, continuation lines get `· `.
/// Handles scroll offset for inputs exceeding the visible area.
pub fn render_input(frame: &mut Frame, input: &InputState, area: Rect) {
    let lines_data = input.lines_with_offsets();
    let visible_start = input.scroll;
    let visible_end = (visible_start + area.height as usize).min(lines_data.len());

    let (cursor_row, cursor_col) = input.row_col();

    for (vi, li) in (visible_start..visible_end).enumerate() {
        let (line_text, _) = lines_data[li];
        let y = area.y + vi as u16;

        let prompt_span = if li == 0 {
            Span::styled(
                "❯ ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("· ", Style::default().fg(Color::DarkGray))
        };

        let text_span = Span::raw(line_text.to_string());
        let line = Line::from(vec![prompt_span, text_span]);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, Rect::new(area.x, y, area.width, 1));
    }

    // Position cursor if visible
    if cursor_row >= visible_start && cursor_row < visible_end {
        let screen_row = (cursor_row - visible_start) as u16;
        let cursor_x = area.x + 2 + cursor_col as u16;
        let cursor_y = area.y + screen_row;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Returns the number of lines the input area needs (for layout calculation).
pub fn input_height(input: &InputState) -> u16 {
    input.visible_line_count() as u16
}
