use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::render::highlight_code;

// ─── MarkdownCollector ────────────────────────────────────────────────────────

/// Collecte les deltas de texte en streaming et en extrait des lignes ratatui.
///
/// - Chaque ligne complète (terminée par `\n`) est immédiatement rendue.
/// - Les blocs de code ``` sont tamponnés jusqu'à la clôture pour être
///   rendus d'un seul tenant avec coloration syntaxique.
/// - Le texte partiel en cours reste dans `pending` jusqu'au `flush_pending()`.
pub struct MarkdownCollector {
    pending: String,
    /// Tampon actif pour un bloc ``` : `(lang, lignes_accumulées)`.
    code_fence: Option<(String, String)>,
}

impl MarkdownCollector {
    pub fn new() -> Self {
        Self { pending: String::new(), code_fence: None }
    }

    pub fn push_delta(&mut self, delta: &str) -> Vec<Line<'static>> {
        self.pending.push_str(delta);
        let mut lines = Vec::new();
        while let Some(pos) = self.pending.find('\n') {
            let raw = self.pending[..pos].to_owned();
            self.pending = self.pending[pos + 1..].to_owned();
            lines.extend(self.process_line(raw));
        }
        lines
    }

    /// Flush le texte partiel restant en fin de stream.
    pub fn flush_pending(&mut self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if let Some((lang, buf)) = self.code_fence.take() {
            lines.extend(render_code_block(&buf, &lang));
        }
        if !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            lines.extend(render_inline_line(&text));
        }
        lines
    }

    fn process_line(&mut self, line: String) -> Vec<Line<'static>> {
        if let Some((_, buf)) = &mut self.code_fence {
            if is_fence_marker(&line) {
                let (lang, code) = self.code_fence.take().unwrap();
                return render_code_block(&code, &lang);
            }
            buf.push_str(&line);
            buf.push('\n');
            return vec![];
        }

        if is_fence_marker(&line) {
            let lang = line
                .trim_start_matches('`')
                .trim_start_matches('~')
                .trim()
                .to_owned();
            self.code_fence = Some((lang, String::new()));
            return vec![];
        }

        if line.is_empty() {
            return vec![Line::default()];
        }

        render_inline_line(&line)
    }
}

impl Default for MarkdownCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Rendu ────────────────────────────────────────────────────────────────────

fn is_fence_marker(line: &str) -> bool {
    let s = line.trim_start();
    s.starts_with("```") || s.starts_with("~~~")
}

/// Rendu d'une ligne unique via le renderer pulldown-cmark complet.
/// Gère headings, bold, italic, listes, links, inline code.
fn render_inline_line(line: &str) -> Vec<Line<'static>> {
    let mut lines = render_markdown(line, 120);
    // Supprimer les lignes vides de fin générées par les tags de bloc.
    while lines.last().is_some_and(|l: &Line<'_>| l.spans.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn render_code_block(code: &str, lang: &str) -> Vec<Line<'static>> {
    const W: usize = 78;
    let border_style = Style::default().fg(Color::DarkGray);
    let top = format!("┌{:─<W$}┐", "");
    let bot = format!("└{:─<W$}┘", "");
    let mut lines = vec![Line::from(Span::styled(top, border_style))];

    let code = code.trim_end_matches('\n');
    let highlighted = (!lang.is_empty())
        .then(|| highlight_code(code, lang))
        .flatten();

    if let Some(hl_lines) = highlighted {
        for hl_line in hl_lines {
            let content_len: usize = hl_line.spans.iter().map(|s| s.content.len()).sum();
            let pad = " ".repeat(W.saturating_sub(2).saturating_sub(content_len));
            let mut spans = vec![Span::styled("│ ".to_owned(), border_style)];
            spans.extend(hl_line.spans);
            spans.push(Span::raw(pad));
            spans.push(Span::styled("│".to_owned(), border_style));
            lines.push(Line::from(spans));
        }
    } else {
        for src_line in code.lines() {
            let content_len = src_line.len();
            let pad = " ".repeat(W.saturating_sub(2).saturating_sub(content_len));
            lines.push(Line::from(Span::styled(
                format!("│ {src_line}{pad}│"),
                Style::default().fg(Color::White),
            )));
        }
    }

    lines.push(Line::from(Span::styled(bot, border_style)));
    lines
}

/// Rendu Markdown complet d'un bloc de texte via pulldown-cmark.
pub fn render_markdown(text: &str, width: u16) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(text, options);
    let mut renderer = MarkdownRenderer::new(width);
    renderer.render(parser);
    renderer.into_lines()
}

// ─── MarkdownRenderer ─────────────────────────────────────────────────────────

struct MarkdownRenderer {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    indent: usize,
    list_depth: u32,
    list_item_index: Vec<Option<u64>>,
    in_code_block: bool,
    code_lang: String,
    code_buf: String,
    width: u16,
    style_stack: Vec<Style>,
}

impl MarkdownRenderer {
    fn new(width: u16) -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            indent: 0,
            list_depth: 0,
            list_item_index: Vec::new(),
            in_code_block: false,
            code_lang: String::new(),
            code_buf: String::new(),
            width,
            style_stack: vec![Style::default()],
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, s: Style) {
        let merged = self.current_style().patch(s);
        self.style_stack.push(merged);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn commit_line(&mut self) {
        let spans = std::mem::take(&mut self.current_spans);
        self.lines.push(Line::from(spans));
    }

    fn push_span(&mut self, text: impl Into<String>) {
        let style = self.current_style();
        self.current_spans.push(Span::styled(text.into(), style));
    }

    fn render(&mut self, parser: Parser) {
        for event in parser {
            match event {
                Event::Start(tag) => self.on_tag_start(tag),
                Event::End(tag) => self.on_tag_end(tag),
                Event::Text(text) => {
                    if self.in_code_block {
                        self.code_buf.push_str(&text);
                    } else {
                        self.push_span(text.into_string());
                    }
                }
                Event::Code(code) => {
                    self.push_style(Style::default().fg(Color::Cyan));
                    self.push_span(format!("`{}`", code.as_ref()));
                    self.pop_style();
                }
                Event::SoftBreak => self.push_span(" "),
                Event::HardBreak => self.commit_line(),
                Event::Rule => {
                    let w = (self.width as usize).saturating_sub(2).max(4);
                    self.current_spans.push(Span::styled(
                        "─".repeat(w),
                        Style::default().fg(Color::DarkGray),
                    ));
                    self.commit_line();
                }
                _ => {}
            }
        }
        if !self.current_spans.is_empty() {
            self.commit_line();
        }
    }

    fn on_tag_start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.push_style(heading_style(level));
            }
            Tag::BlockQuote(_) => {
                self.indent += 2;
                self.push_style(Style::default().fg(Color::DarkGray));
                self.current_spans.push(Span::styled(
                    "│ ".to_owned(),
                    Style::default().fg(Color::Blue),
                ));
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                use pulldown_cmark::CodeBlockKind;
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_buf.clear();
            }
            Tag::List(start) => {
                self.list_depth += 1;
                self.list_item_index.push(start);
            }
            Tag::Item => {
                let indent = "  ".repeat((self.list_depth as usize).saturating_sub(1));
                let bullet = match self.list_item_index.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{n}. ");
                        *n += 1;
                        s
                    }
                    _ => "• ".to_owned(),
                };
                self.current_spans.push(Span::styled(
                    format!("{indent}{bullet}"),
                    Style::default().fg(Color::Cyan),
                ));
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.push_style(
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                );
                let _ = dest_url;
            }
            Tag::Paragraph | Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            _ => {}
        }
    }

    fn on_tag_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.pop_style();
                self.commit_line();
                self.lines.push(Line::default());
            }
            TagEnd::Paragraph => {
                if !self.current_spans.is_empty() {
                    self.commit_line();
                }
                self.lines.push(Line::default());
            }
            TagEnd::BlockQuote(_) => {
                self.indent = self.indent.saturating_sub(2);
                self.pop_style();
                if !self.current_spans.is_empty() {
                    self.commit_line();
                }
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                let code = std::mem::take(&mut self.code_buf);
                let lang = std::mem::take(&mut self.code_lang);
                let w = (self.width as usize).saturating_sub(4).max(4);
                let border_style = Style::default().fg(Color::DarkGray);
                let top = format!("┌{:─<w$}┐", "");
                let bot = format!("└{:─<w$}┘", "");
                self.lines.push(Line::from(Span::styled(top, border_style)));

                let highlighted = (!lang.is_empty())
                    .then(|| highlight_code(&code, &lang))
                    .flatten();

                if let Some(hl_lines) = highlighted {
                    for hl_line in hl_lines {
                        let content_len: usize =
                            hl_line.spans.iter().map(|s| s.content.len()).sum();
                        let pad = " ".repeat(w.saturating_sub(3).saturating_sub(content_len));
                        let mut spans = vec![Span::styled("│ ".to_owned(), border_style)];
                        spans.extend(hl_line.spans);
                        spans.push(Span::raw(pad));
                        spans.push(Span::styled("│".to_owned(), border_style));
                        self.lines.push(Line::from(spans));
                    }
                } else {
                    for src_line in code.lines() {
                        let display =
                            format!("│ {:<w$}│", src_line, w = w.saturating_sub(1));
                        self.lines.push(Line::from(Span::styled(
                            display,
                            Style::default().fg(Color::White),
                        )));
                    }
                }

                self.lines.push(Line::from(Span::styled(bot, border_style)));
            }
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                self.list_item_index.pop();
                if self.list_depth == 0 {
                    self.lines.push(Line::default());
                }
            }
            TagEnd::Item if !self.current_spans.is_empty() => {
                self.commit_line();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style();
            }
            _ => {}
        }
    }

    fn into_lines(self) -> Vec<Line<'static>> {
        self.lines
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().add_modifier(Modifier::BOLD),
    }
}
