use std::any::Any;

use ratatui::text::Line;

/// Abstraction commune à tous les types de contenu dans le transcript.
/// Chaque variante (message utilisateur, réponse assistant, appel outil…)
/// implémente ce trait.
///
/// La borne `'static` est requise pour le downcasting via `Any`.
pub trait HistoryCell: std::fmt::Debug + Send + Sync + 'static {
    /// Lignes à afficher dans le transcript, calculées pour la largeur donnée.
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    /// Hauteur souhaitée en lignes terminales pour la largeur donnée.
    #[allow(dead_code)]
    fn desired_height(&self, width: u16) -> u16 {
        self.display_lines(width).len() as u16
    }

    /// Appelé à chaque tick d'animation (~80 ms par défaut).
    /// Retourne `true` si un redraw est nécessaire (spinner, streaming, etc.).
    fn tick(&mut self) -> bool {
        false
    }

    /// Downcasting : retourne `self` en tant que `&mut dyn Any`.
    /// Chaque implémentation doit retourner `self` directement.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
