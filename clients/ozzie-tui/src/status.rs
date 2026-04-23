use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Barre de statut d'une ligne affichée tout en bas.
pub struct StatusLine {
    pub conversation_id: String,
    pub agent_running: bool,
    pub active_tool: Option<String>,
}

impl StatusLine {
    pub fn render(&self) -> Paragraph<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Conversation ID
        spans.push(Span::styled(
            format!(" [{}] ", self.conversation_id),
            Style::default().fg(Color::DarkGray),
        ));

        // Tool actif
        if let Some(ref tool) = self.active_tool {
            spans.push(Span::styled(
                format!("⚙ {tool} "),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }

        // État agent
        if self.agent_running {
            spans.push(Span::styled(
                "● ".to_owned(),
                Style::default().fg(Color::Green),
            ));
            spans.push(Span::styled(
                "Ctrl+C interrompre  ".to_owned(),
                Style::default().fg(Color::DarkGray),
            ));
        }

        // Séparateur + raccourcis
        spans.push(Span::styled(
            "Ctrl+Q quitter".to_owned(),
            Style::default().fg(Color::DarkGray),
        ));

        Paragraph::new(Line::from(spans))
    }
}
