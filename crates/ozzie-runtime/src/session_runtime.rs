use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

use crate::react::PendingDrain;

/// Per-session runtime state for the ReactLoop.
///
/// Tracks whether a loop is active, buffers pending user messages,
/// and holds a cancellation token for explicit stop signals.
pub struct SessionRuntime {
    /// Cancellation token — triggered by `ctrl+c` or `/stop`.
    /// Behind Mutex so it can be replaced on reset via `&self`.
    cancel_token: Mutex<CancellationToken>,
    /// User messages received while the loop is active.
    pending: Mutex<Vec<String>>,
    /// Whether a ReactLoop is currently running for this session.
    active: AtomicBool,
}

impl SessionRuntime {
    pub fn new() -> Self {
        Self {
            cancel_token: Mutex::new(CancellationToken::new()),
            pending: Mutex::new(Vec::new()),
            active: AtomicBool::new(false),
        }
    }

    /// Returns true if a ReactLoop is currently active for this session.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Marks the session as active (a ReactLoop is running).
    pub fn set_active(&self, v: bool) {
        self.active.store(v, Ordering::Release);
    }

    /// Buffers a user message to be drained at the next turn.
    pub fn push_pending(&self, text: String) {
        self.pending.lock().unwrap().push(text);
    }

    /// Returns a clone of the current cancellation token.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.lock().unwrap().clone()
    }

    /// Cancels the current loop.
    pub fn cancel(&self) {
        self.cancel_token.lock().unwrap().cancel();
    }

    /// Resets the session runtime for a new loop iteration.
    ///
    /// Creates a fresh cancellation token and clears pending messages.
    /// Called after a loop completes or is cancelled.
    pub fn reset(&self) {
        *self.cancel_token.lock().unwrap() = CancellationToken::new();
        self.pending.lock().unwrap().clear();
        self.active.store(false, Ordering::Release);
    }
}

impl Default for SessionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingDrain for SessionRuntime {
    fn drain(&self) -> Vec<String> {
        std::mem::take(&mut *self.pending.lock().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_runtime_is_inactive() {
        let rt = SessionRuntime::new();
        assert!(!rt.is_active());
        assert!(!rt.cancel_token().is_cancelled());
        assert!(rt.drain().is_empty());
    }

    #[test]
    fn push_and_drain_pending() {
        let rt = SessionRuntime::new();
        rt.push_pending("msg1".to_string());
        rt.push_pending("msg2".to_string());

        let drained = rt.drain();
        assert_eq!(drained, vec!["msg1", "msg2"]);

        // Second drain is empty
        assert!(rt.drain().is_empty());
    }

    #[test]
    fn active_flag() {
        let rt = SessionRuntime::new();
        assert!(!rt.is_active());

        rt.set_active(true);
        assert!(rt.is_active());

        rt.set_active(false);
        assert!(!rt.is_active());
    }

    #[test]
    fn cancel_token_propagation() {
        let rt = SessionRuntime::new();
        let token = rt.cancel_token();

        assert!(!token.is_cancelled());
        rt.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn reset_creates_fresh_state() {
        let rt = SessionRuntime::new();
        rt.set_active(true);
        rt.push_pending("msg".to_string());
        rt.cancel();

        assert!(rt.is_active());
        assert!(rt.cancel_token().is_cancelled());

        rt.reset();

        assert!(!rt.is_active());
        assert!(!rt.cancel_token().is_cancelled());
        assert!(rt.drain().is_empty());
    }

    #[test]
    fn reset_after_cancel_allows_new_token() {
        let rt = SessionRuntime::new();
        let old_token = rt.cancel_token();
        rt.cancel();
        assert!(old_token.is_cancelled());

        rt.reset();
        let new_token = rt.cancel_token();
        assert!(!new_token.is_cancelled());
        // Old token stays cancelled
        assert!(old_token.is_cancelled());
    }
}
