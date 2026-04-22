use std::sync::OnceLock;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

const MAX_BYTES: usize = 100 * 1024; // 100 KB
const MAX_LINES: usize = 2_000;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes
            .get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
            .cloned()
            .unwrap_or_default()
    })
}

/// Colore `code` selon `lang` et retourne les lignes ratatui.
/// Retourne `None` si la langue n'est pas reconnue ou si le code est trop grand.
pub fn highlight_code(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if code.len() > MAX_BYTES || code.lines().count() > MAX_LINES {
        return None;
    }

    let ss = syntax_set();
    let syntax = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))?;

    let mut h = HighlightLines::new(syntax, theme());
    let mut result = Vec::new();

    for line_str in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line_str, ss).ok()?;
        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = syntect_color_to_ratatui(style.foreground);
                Span::styled(text.trim_end_matches('\n').to_owned(), Style::default().fg(fg))
            })
            .collect();
        result.push(Line::from(spans));
    }

    Some(result)
}

fn syntect_color_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
