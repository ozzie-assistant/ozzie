use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use ozzie_core::conscience::ApprovalResponse;

use crate::app::ActivePrompt;

/// Renders the dangerous tool approval prompt bar.
pub fn render_prompt(frame: &mut Frame, prompt: &ActivePrompt, area: Rect) {
    let mut spans = vec![
        Span::styled(
            " ⚠ ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", prompt.label),
            Style::default().fg(Color::Yellow),
        ),
    ];

    for (i, option) in ApprovalResponse::ALL.iter().enumerate() {
        let option = option.label();
        spans.push(Span::raw(" "));
        if i == prompt.selected {
            spans.push(Span::styled(
                format!(" {option} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {option} "),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    spans.push(Span::styled(
        "  ←/→ select  Enter confirm",
        Style::default().fg(Color::DarkGray),
    ));

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}
