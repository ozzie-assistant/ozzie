use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{ConnectionState, SPINNER_FRAMES};

/// Renders the status bar into the given area.
pub fn render_status(
    frame: &mut Frame,
    connection: ConnectionState,
    session_id: Option<&str>,
    status: &str,
    working_since: Option<std::time::Instant>,
    spinner_frame: usize,
    area: Rect,
) {
    let (state_str, state_color) = match connection {
        ConnectionState::Connected => ("CONNECTED", Color::Green),
        ConnectionState::Connecting => ("CONNECTING...", Color::Yellow),
        ConnectionState::Reconnecting => ("RECONNECTING...", Color::Yellow),
        ConnectionState::Disconnected => ("DISCONNECTED", Color::Red),
    };

    let session = session_id.unwrap_or("-");

    let mut spans = vec![
        Span::styled(
            format!(" {state_str} "),
            Style::default().fg(Color::Black).bg(state_color),
        ),
        Span::raw(format!("  session: {session}  ")),
    ];

    // Spinner + elapsed time when working
    if let Some(since) = working_since {
        let elapsed = since.elapsed().as_secs();
        let elapsed_str = if elapsed < 60 {
            format!("{elapsed}s")
        } else {
            format!("{}m {}s", elapsed / 60, elapsed % 60)
        };
        let frame_char = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
        spans.push(Span::styled(
            format!("{frame_char} Working... {elapsed_str}  "),
            Style::default().fg(Color::Cyan),
        ));
    }

    spans.push(Span::styled(status, Style::default().fg(Color::DarkGray)));
    spans.push(Span::raw("  ^C quit | ⇧↵ newline | Tab tool | ↑↓ scroll"));

    let status_line = Line::from(spans);
    let paragraph = Paragraph::new(status_line);
    frame.render_widget(paragraph, area);
}
