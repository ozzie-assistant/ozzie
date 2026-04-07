use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Renders markdown text into styled ratatui Lines.
pub fn render_markdown(text: &str, indent: &str) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut renderer = MdRenderer {
        indent: indent.to_string(),
        lines: Vec::new(),
        current_spans: Vec::new(),
        style_stack: Vec::new(),
        in_code_block: false,
        in_heading: false,
        heading_level: 0,
        list_stack: Vec::new(),
        in_block_quote: false,
        link_url: None,
    };

    for event in parser {
        renderer.process(event);
    }

    // Flush remaining spans
    renderer.flush_line();

    renderer.lines
}

/// List context for nested list rendering.
enum ListKind {
    Unordered,
    Ordered(u64),
}

struct MdRenderer {
    indent: String,
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    in_heading: bool,
    heading_level: u8,
    list_stack: Vec<ListKind>,
    in_block_quote: bool,
    link_url: Option<String>,
}

impl MdRenderer {
    fn current_style(&self) -> Style {
        self.style_stack
            .last()
            .copied()
            .unwrap_or(Style::default())
    }

    fn push_style(&mut self, style: Style) {
        let merged = self.current_style().patch(style);
        self.style_stack.push(merged);
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
    }

    fn flush_line(&mut self) {
        if !self.current_spans.is_empty() {
            let spans = std::mem::take(&mut self.current_spans);
            self.lines.push(Line::from(spans));
        }
    }

    fn add_text(&mut self, text: &str) {
        if self.in_code_block {
            // Code blocks: emit each line with prefix
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    self.flush_line();
                }
                if i > 0 || self.current_spans.is_empty() {
                    self.current_spans.push(Span::styled(
                        format!("{}│ ", self.indent),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                self.current_spans.push(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
            }
            return;
        }

        let style = self.current_style();
        let prefix = self.line_prefix();

        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.flush_line();
            }
            // Add prefix for new lines (first line gets prefix from start event)
            if i > 0 && !prefix.is_empty() {
                self.current_spans
                    .push(Span::styled(prefix.clone(), Style::default().fg(Color::DarkGray)));
            }
            if !line.is_empty() {
                self.current_spans
                    .push(Span::styled(line.to_string(), style));
            }
        }
    }

    fn line_prefix(&self) -> String {
        if self.in_block_quote {
            format!("{}▎ ", self.indent)
        } else {
            String::new()
        }
    }

    fn process(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.add_text(&text),
            Event::Code(code) => {
                self.current_spans.push(Span::styled(
                    format!("`{code}`"),
                    self.current_style().fg(Color::Yellow),
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                self.flush_line();
            }
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if self.in_block_quote {
                    self.current_spans.push(Span::styled(
                        format!("{}▎ ", self.indent),
                        Style::default().fg(Color::DarkGray),
                    ));
                } else if !self.list_stack.is_empty() {
                    // list item paragraph — prefix already handled
                } else {
                    self.current_spans
                        .push(Span::raw(self.indent.clone()));
                }
            }
            Tag::Heading { level, .. } => {
                self.in_heading = true;
                self.heading_level = level as u8;
                let color = if level == pulldown_cmark::HeadingLevel::H1 {
                    Color::Cyan
                } else {
                    Color::Blue
                };
                self.push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
                self.current_spans
                    .push(Span::raw(self.indent.clone()));
            }
            Tag::Strong => {
                self.push_style(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Emphasis => {
                self.push_style(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::CodeBlock(_) => {
                self.flush_line();
                self.in_code_block = true;
            }
            Tag::List(start) => {
                if !self.list_stack.is_empty() {
                    self.flush_line();
                }
                match start {
                    Some(n) => self.list_stack.push(ListKind::Ordered(n)),
                    None => self.list_stack.push(ListKind::Unordered),
                }
            }
            Tag::Item => {
                self.flush_line();
                let depth = self.list_stack.len().saturating_sub(1);
                let list_indent = "  ".repeat(depth);
                let bullet = match self.list_stack.last_mut() {
                    Some(ListKind::Unordered) => "• ".to_string(),
                    Some(ListKind::Ordered(n)) => {
                        let s = format!("{n}. ");
                        *n += 1;
                        s
                    }
                    None => "• ".to_string(),
                };
                self.current_spans
                    .push(Span::raw(format!("{}{list_indent}", self.indent)));
                self.current_spans.push(Span::styled(
                    bullet,
                    Style::default().fg(Color::LightBlue),
                ));
            }
            Tag::BlockQuote(_) => {
                self.flush_line();
                self.in_block_quote = true;
                self.push_style(Style::default().fg(Color::DarkGray));
            }
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                if !self.in_block_quote && self.list_stack.is_empty() {
                    self.lines.push(Line::from(""));
                }
            }
            TagEnd::Heading(_) => {
                self.in_heading = false;
                self.heading_level = 0;
                self.pop_style();
                self.flush_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                self.in_code_block = false;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.flush_line();
                    self.lines.push(Line::from(""));
                }
            }
            TagEnd::Item => {
                self.flush_line();
            }
            TagEnd::BlockQuote(_) => {
                self.in_block_quote = false;
                self.pop_style();
                self.flush_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::Link => {
                if let Some(url) = self.link_url.take() {
                    self.current_spans.push(Span::styled(
                        format!(" ({url})"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn bold_text() {
        let lines = render_markdown("**hello**", "");
        assert!(!lines.is_empty());
        let text = line_text(&lines[0]);
        assert!(text.contains("hello"));
        // Check bold modifier
        let has_bold = lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold);
    }

    #[test]
    fn italic_text() {
        let lines = render_markdown("*italic*", "");
        let has_italic = lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::ITALIC));
        assert!(has_italic);
    }

    #[test]
    fn inline_code() {
        let lines = render_markdown("use `code` here", "");
        let text = line_text(&lines[0]);
        assert!(text.contains("`code`"));
    }

    #[test]
    fn code_block() {
        let md = "```\nlet x = 1;\nlet y = 2;\n```";
        let lines = render_markdown(md, "  ");
        let all_text: String = lines.iter().map(|l| line_text(l)).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("let x = 1;"));
        assert!(all_text.contains("│"));
    }

    #[test]
    fn heading_h1() {
        let lines = render_markdown("# Title", "");
        let has_bold = lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold);
    }

    #[test]
    fn unordered_list() {
        let md = "- one\n- two";
        let lines = render_markdown(md, "  ");
        let all_text: String = lines.iter().map(|l| line_text(l)).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("•"));
        assert!(all_text.contains("one"));
        assert!(all_text.contains("two"));
    }

    #[test]
    fn ordered_list() {
        let md = "1. first\n2. second";
        let lines = render_markdown(md, "  ");
        let all_text: String = lines.iter().map(|l| line_text(l)).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("1."));
        assert!(all_text.contains("2."));
    }

    #[test]
    fn blockquote() {
        let lines = render_markdown("> quoted text", "  ");
        let all_text: String = lines.iter().map(|l| line_text(l)).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("▎"));
        assert!(all_text.contains("quoted text"));
    }

    #[test]
    fn strikethrough() {
        let lines = render_markdown("~~strike~~", "");
        let has_crossed = lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert!(has_crossed);
    }

    #[test]
    fn link_rendering() {
        let lines = render_markdown("[click](https://example.com)", "");
        let text = line_text(&lines[0]);
        assert!(text.contains("click"));
        assert!(text.contains("https://example.com"));
    }

    #[test]
    fn with_indent() {
        let lines = render_markdown("hello", "  ");
        let text = line_text(&lines[0]);
        assert!(text.starts_with("  "));
    }
}
