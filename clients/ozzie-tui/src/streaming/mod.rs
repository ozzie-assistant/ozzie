pub mod markdown;

use std::collections::VecDeque;

use ratatui::text::Line;

use crate::transcript::AssistantCell;

use markdown::MarkdownCollector;

/// Gère le pipeline de streaming texte → lignes ratatui animées.
///
/// Principe : les deltas arrivent à n'importe quelle fréquence, mais les
/// lignes ne sont émises vers `AssistantCell` qu'une par `on_tick()`.
/// Cela crée l'effet d'écriture en live sans écraser le terminal.
pub struct StreamController {
    collector: MarkdownCollector,
    /// Lignes complètes prêtes à être émises.
    queue: VecDeque<Line<'static>>,
    /// Indique que `finalize()` a été appelé — on draine tout.
    finishing: bool,
}

impl StreamController {
    pub fn new() -> Self {
        Self {
            collector: MarkdownCollector::new(),
            queue: VecDeque::new(),
            finishing: false,
        }
    }

    /// Pousse un delta de texte brut.
    pub fn push(&mut self, delta: &str) {
        let new_lines = self.collector.push_delta(delta);
        self.queue.extend(new_lines);
    }

    /// Appelé à chaque tick. Émet N lignes vers `cell` selon la vitesse
    /// du backend. Retourne `true` si du contenu a été émis.
    pub fn on_tick(&mut self, cell: &mut AssistantCell) -> bool {
        if self.queue.is_empty() {
            return false;
        }

        // Drain adaptatif : si le queue est grand on émet plus vite.
        let batch = match self.queue.len() {
            0 => return false,
            1..=5 => 1,
            6..=20 => 2,
            _ => 4,
        };

        for line in self.queue.drain(..batch.min(self.queue.len())) {
            cell.push_line(line);
        }
        true
    }

    /// Signale la fin du stream. Le prochain `on_tick` drainera tout.
    pub fn finalize(&mut self, cell: &mut AssistantCell) {
        // Flush le texte partiel restant dans le collector.
        for line in self.collector.flush_pending() {
            self.queue.push_back(line);
        }
        // Drain tout immédiatement.
        for line in self.queue.drain(..) {
            cell.push_line(line);
        }
        cell.finalize();
        self.finishing = true;
    }

    #[allow(dead_code)]
    pub fn is_done(&self) -> bool {
        self.finishing && self.queue.is_empty()
    }
}

impl Default for StreamController {
    fn default() -> Self {
        Self::new()
    }
}
