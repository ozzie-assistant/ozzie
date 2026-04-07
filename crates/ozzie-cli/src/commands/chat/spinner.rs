//! Animated spinner for active tool calls.
//!
//! `tick()` and `is_active()` will be used in Phase 3 (async select loop).

use std::io::{self, Write};
use std::time::Instant;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Tracks a single active tool call spinner.
struct ActiveCall {
    name: String,
    /// Used by `tick()` for animation (Phase 3: async select loop).
    #[allow(dead_code)]
    frame: usize,
    start: Instant,
}

/// Manages spinners for concurrent tool calls.
pub struct SpinnerSet {
    active: Vec<ActiveCall>,
}

impl SpinnerSet {
    pub fn new() -> Self {
        Self { active: Vec::new() }
    }

    /// Starts a spinner for a tool call.
    pub fn start(&mut self, name: &str) {
        // Print initial line
        let frame = FRAMES[0];
        eprint!("  \x1b[34m{frame} {name}\x1b[0m");
        let _ = io::stderr().flush();

        self.active.push(ActiveCall {
            name: name.to_string(),
            frame: 0,
            start: Instant::now(),
        });
    }

    /// Completes a tool call spinner with success/error.
    pub fn finish(&mut self, name: &str, is_error: bool) {
        if let Some(pos) = self.active.iter().position(|c| c.name == name) {
            let call = self.active.remove(pos);
            let elapsed = call.start.elapsed();
            let ms = elapsed.as_millis();

            // Clear current line and print result
            let (icon, color) = if is_error {
                ("✘", "\x1b[31m")
            } else {
                ("✔", "\x1b[32m")
            };

            let duration = if ms < 1000 {
                format!("{ms}ms")
            } else {
                format!("{:.1}s", elapsed.as_secs_f64())
            };

            eprint!("\r\x1b[2K  {color}{icon} {name}\x1b[0m \x1b[90m{duration}\x1b[0m\n");
            let _ = io::stderr().flush();
        }
    }

    /// Ticks all active spinners (call ~every 80ms).
    /// Used in Phase 3 with async select loop.
    #[allow(dead_code)]
    pub fn tick(&mut self) {
        if self.active.is_empty() {
            return;
        }

        // Only tick the last active spinner (the visible one).
        if let Some(call) = self.active.last_mut() {
            call.frame += 1;
            let frame = FRAMES[call.frame % FRAMES.len()];
            eprint!("\r\x1b[2K  \x1b[34m{frame} {}\x1b[0m", call.name);
            let _ = io::stderr().flush();
        }
    }

    /// Returns whether any spinners are active.
    /// Used in Phase 3 with async select loop.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_finish() {
        let mut set = SpinnerSet::new();
        assert!(!set.is_active());

        set.start("shell_exec");
        assert!(set.is_active());

        set.finish("shell_exec", false);
        assert!(!set.is_active());
    }

    #[test]
    fn finish_unknown_is_noop() {
        let mut set = SpinnerSet::new();
        set.finish("nonexistent", false); // should not panic
    }

    #[test]
    fn multiple_concurrent() {
        let mut set = SpinnerSet::new();
        set.start("tool_a");
        set.start("tool_b");
        assert_eq!(set.active.len(), 2);

        set.finish("tool_a", false);
        assert_eq!(set.active.len(), 1);

        set.finish("tool_b", true);
        assert!(!set.is_active());
    }

    #[test]
    fn tick_advances_frame() {
        let mut set = SpinnerSet::new();
        set.start("test");
        let initial_frame = set.active[0].frame;
        set.tick();
        assert_eq!(set.active[0].frame, initial_frame + 1);
    }
}
