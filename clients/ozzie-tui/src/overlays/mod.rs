pub mod approval;

pub use approval::ApprovalOverlay;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

/// Action retournée par `handle_key`.
pub enum ViewAction {
    /// Touche consommée, pas d'effet extérieur.
    Consumed,
    /// L'overlay a terminé, retirer du stack.
    /// Payload optionnel (ex : décision d'approbation).
    Done(Option<String>),
}

/// Trait commun à tous les overlays qui remplacent le composer.
pub trait Overlay: std::fmt::Debug + Send + Sync {
    fn handle_key(&mut self, key: KeyEvent) -> ViewAction;
    fn render(&self, frame: &mut Frame, area: Rect);
    /// Jeton associé à cet overlay (pour le routing des réponses).
    fn token(&self) -> &str;
}
