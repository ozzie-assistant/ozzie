use std::time::Instant;

use ratatui::style::Color;

/// Calcule la couleur du shimmer pour un tick donné.
///
/// Oscillation sinusoïdale entre `base` et `highlight` sur une période de ~800 ms.
#[derive(Debug)]
pub struct Shimmer {
    start: Instant,
}

impl Shimmer {
    pub fn new() -> Self {
        Self { start: Instant::now() }
    }

    /// Couleur interpolée selon le temps écoulé.
    pub fn color(&self) -> Color {
        let elapsed = self.start.elapsed().as_millis() as f64;
        // Période de 800 ms, sin oscille entre -1 et 1 → normalise en [0, 1]
        let t = ((elapsed / 800.0 * std::f64::consts::TAU).sin() + 1.0) / 2.0;
        lerp_color((80, 80, 80), (200, 200, 255), t)
    }
}

impl Default for Shimmer {
    fn default() -> Self {
        Self::new()
    }
}

fn lerp_color(base: (u8, u8, u8), highlight: (u8, u8, u8), t: f64) -> Color {
    let r = lerp_u8(base.0, highlight.0, t);
    let g = lerp_u8(base.1, highlight.1, t);
    let b = lerp_u8(base.2, highlight.2, t);
    Color::Rgb(r, g, b)
}

fn lerp_u8(a: u8, b: u8, t: f64) -> u8 {
    let a = a as f64;
    let b = b as f64;
    (a + (b - a) * t.clamp(0.0, 1.0)) as u8
}
