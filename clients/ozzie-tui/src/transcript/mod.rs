pub mod cell;
pub mod user;
pub mod assistant;
pub mod tool;
pub mod approval;

pub use cell::HistoryCell;
pub use user::UserCell;
pub use assistant::AssistantCell;
pub use tool::ToolCell;
pub use approval::ApprovalCell;

use std::cell::Cell;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

/// Affiche la liste des cellules avec défilement vertical.
pub struct TranscriptWidget {
    cells: Vec<Box<dyn HistoryCell>>,
    /// Distance en lignes depuis le bas (0 = vue sur la fin du transcript).
    scroll_offset: usize,
    /// Si `true`, le scroll est ancré au bas (auto-scroll).
    pinned_to_bottom: bool,
    /// Dernier `max_offset` calculé à l'affichage — permet de normaliser
    /// `scroll_offset` sans connaître la hauteur dans `scroll()`.
    cached_max_offset: Cell<usize>,
}

impl TranscriptWidget {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            scroll_offset: 0,
            pinned_to_bottom: true,
            cached_max_offset: Cell::new(0),
        }
    }

    pub fn push(&mut self, cell: Box<dyn HistoryCell>) {
        self.cells.push(cell);
        if self.pinned_to_bottom {
            self.scroll_offset = 0;
        }
    }

    /// Itérateur mutable sur les cellules, du plus récent au plus ancien.
    pub fn cells_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Box<dyn HistoryCell>> {
        self.cells.iter_mut()
    }

    /// Tick d'animation : retourne `true` si un redraw est nécessaire.
    pub fn tick(&mut self) -> bool {
        self.cells.iter_mut().any(|c| c.tick())
    }

    /// Défilement vers le bas (valeur positive) ou vers le haut (négative).
    pub fn scroll(&mut self, delta: i32) {
        if delta < 0 {
            let up = (-delta) as usize;
            self.scroll_offset = self.scroll_offset.saturating_add(up);
            self.pinned_to_bottom = false;
        } else {
            let down = delta as usize;
            // Normaliser : si scroll_offset dépasse le max réel (contenu réduit ou
            // jamais atteint via render), on part du max pour éviter d'être "coincé".
            let effective = self.scroll_offset.min(self.cached_max_offset.get());
            if effective > down {
                self.scroll_offset = effective - down;
            } else {
                self.scroll_offset = 0;
                self.pinned_to_bottom = true;
            }
        }
    }

    #[allow(dead_code)]
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.pinned_to_bottom = true;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let width = area.width.saturating_sub(2);
        let block = Block::bordered().title(" Ozzie ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Les lignes sont déjà pré-wrappées à `width` → chaque élément = 1 ligne visuelle.
        let all_lines = self.collect_lines(width);
        let total = all_lines.len();
        let visible = inner.height as usize;

        let max_offset = total.saturating_sub(visible);
        self.cached_max_offset.set(max_offset);

        let start = if self.pinned_to_bottom {
            max_offset
        } else {
            let from_bottom = self.scroll_offset.min(max_offset);
            max_offset - from_bottom
        };

        let visible_lines: Vec<Line> =
            all_lines.into_iter().skip(start).take(visible).collect();

        frame.render_widget(Paragraph::new(visible_lines), inner);

        if total > visible {
            let mut scrollbar_state = ScrollbarState::new(total).position(start);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight),
                area,
                &mut scrollbar_state,
            );
        }
    }

    fn collect_lines(&self, width: u16) -> Vec<Line<'static>> {
        let max_chars = width as usize;
        self.cells
            .iter()
            .flat_map(|c| c.display_lines(width))
            .flat_map(|line| pre_wrap_line(line, max_chars))
            .collect()
    }
}

fn pre_wrap_line(line: Line<'static>, max_chars: usize) -> Vec<Line<'static>> {
    if max_chars == 0 {
        return vec![line];
    }
    let total: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= max_chars {
        return vec![line];
    }
    // Flatten to (char, Style) pairs for uniform handling.
    let styled: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();

    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut pos = 0;
    while pos < styled.len() {
        let end = (pos + max_chars).min(styled.len());
        // Try to break at the last space within the window.
        let break_at = if end < styled.len() {
            styled[pos..end]
                .iter()
                .rposition(|(c, _)| *c == ' ')
                .map(|i| pos + i + 1)
                .filter(|&p| p > pos)
                .unwrap_or(end)
        } else {
            end
        };
        // Re-group consecutive chars with the same style into Spans.
        let row_chars = &styled[pos..break_at];
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut i = 0;
        while i < row_chars.len() {
            let style = row_chars[i].1;
            let run_len = row_chars[i..]
                .iter()
                .position(|(_, s)| *s != style)
                .unwrap_or(row_chars.len() - i);
            let text: String = row_chars[i..i + run_len].iter().map(|(c, _)| *c).collect();
            if !text.is_empty() {
                spans.push(Span::styled(text, style));
            }
            i += run_len;
        }
        rows.push(Line::from(spans));
        pos = break_at;
    }
    rows
}

impl Default for TranscriptWidget {
    fn default() -> Self {
        Self::new()
    }
}
