use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::RwLock;

/// Tracks per-session active tools with thread-safety.
///
/// Core tools are always active; additional tools can be activated at runtime
/// via `activate`. All methods are safe for concurrent use.
pub struct ToolSet {
    inner: RwLock<ToolSetInner>,
}

struct ToolSetInner {
    /// Always-on tools.
    core: HashSet<String>,
    /// session_id -> activated tool names.
    active: HashMap<String, HashSet<String>>,
    /// session_id -> activated-during-turn flag.
    turn_flags: HashMap<String, bool>,
    /// Every known tool name.
    all_names: HashSet<String>,
}

impl ToolSet {
    /// Creates a new ToolSet. `core_tools` are always active for every session.
    /// `all_tools` is the full catalog of known tool names.
    pub fn new(core_tools: &[&str], all_tools: &[&str]) -> Self {
        Self {
            inner: RwLock::new(ToolSetInner {
                core: core_tools.iter().map(|s| s.to_string()).collect(),
                active: HashMap::new(),
                turn_flags: HashMap::new(),
                all_names: all_tools.iter().map(|s| s.to_string()).collect(),
            }),
        }
    }

    /// Returns the sorted list of tool names currently active for the given
    /// session (core + session-specific activations).
    pub fn active_tool_names(&self, session_id: &str) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        let mut set: BTreeSet<&str> = inner.core.iter().map(|s| s.as_str()).collect();
        if let Some(session_tools) = inner.active.get(session_id) {
            for name in session_tools {
                set.insert(name);
            }
        }
        set.into_iter().map(|s| s.to_string()).collect()
    }

    /// Adds a tool to the session's active set.
    /// Returns false if the tool name is not in the known catalog.
    pub fn activate(&self, session_id: &str, tool_name: &str) -> bool {
        let mut inner = self.inner.write().unwrap();
        if !inner.all_names.contains(tool_name) {
            return false;
        }
        inner
            .active
            .entry(session_id.to_string())
            .or_default()
            .insert(tool_name.to_string());
        inner.turn_flags.insert(session_id.to_string(), true);
        true
    }

    /// Returns true if `tool_name` is in the full catalog.
    pub fn is_known(&self, tool_name: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.all_names.contains(tool_name)
    }

    /// Returns true if the tool is currently active for the session
    /// (either core or explicitly activated).
    pub fn is_active(&self, session_id: &str, tool_name: &str) -> bool {
        let inner = self.inner.read().unwrap();
        if inner.core.contains(tool_name) {
            return true;
        }
        inner
            .active
            .get(session_id)
            .is_some_and(|s| s.contains(tool_name))
    }

    /// Clears the activation flag for the current turn.
    pub fn reset_turn_flag(&self, session_id: &str) {
        let mut inner = self.inner.write().unwrap();
        inner.turn_flags.insert(session_id.to_string(), false);
    }

    /// Returns true if any tool was activated since the last
    /// `reset_turn_flag` call for this session.
    pub fn activated_during_turn(&self, session_id: &str) -> bool {
        let inner = self.inner.read().unwrap();
        inner.turn_flags.get(session_id).copied().unwrap_or(false)
    }

    /// Returns true if there are known tools that are not currently
    /// active for the session.
    pub fn has_inactive_tools(&self, session_id: &str) -> bool {
        let inner = self.inner.read().unwrap();
        let mut active_count = inner.core.len();
        if let Some(session_tools) = inner.active.get(session_id) {
            active_count += session_tools.len();
        }
        active_count < inner.all_names.len()
    }

    /// Adds a tool to the core (always-active) set and the full catalog.
    pub fn register_core(&self, name: &str) {
        let mut inner = self.inner.write().unwrap();
        inner.core.insert(name.to_string());
        inner.all_names.insert(name.to_string());
    }

    /// Removes all per-session state for the given session.
    pub fn cleanup(&self, session_id: &str) {
        let mut inner = self.inner.write().unwrap();
        inner.active.remove(session_id);
        inner.turn_flags.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_always_active() {
        let ts = ToolSet::new(&["read", "write"], &["read", "write", "shell", "web"]);
        let names = ts.active_tool_names("s1");
        assert!(names.contains(&"read".to_string()));
        assert!(names.contains(&"write".to_string()));
        assert!(!names.contains(&"shell".to_string()));
    }

    #[test]
    fn activate_known() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        assert!(ts.activate("s1", "shell"));
        let names = ts.active_tool_names("s1");
        assert!(names.contains(&"shell".to_string()));
    }

    #[test]
    fn activate_unknown() {
        let ts = ToolSet::new(&["read"], &["read"]);
        assert!(!ts.activate("s1", "unknown_tool"));
    }

    #[test]
    fn activate_idempotent() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        assert!(ts.activate("s1", "shell"));
        assert!(ts.activate("s1", "shell"));
        let names = ts.active_tool_names("s1");
        assert_eq!(names.iter().filter(|n| *n == "shell").count(), 1);
    }

    #[test]
    fn session_isolation() {
        let ts = ToolSet::new(&["read"], &["read", "shell", "web"]);
        ts.activate("s1", "shell");
        ts.activate("s2", "web");

        let s1 = ts.active_tool_names("s1");
        let s2 = ts.active_tool_names("s2");

        assert!(s1.contains(&"shell".to_string()));
        assert!(!s1.contains(&"web".to_string()));
        assert!(s2.contains(&"web".to_string()));
        assert!(!s2.contains(&"shell".to_string()));
    }

    #[test]
    fn turn_flags() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        assert!(!ts.activated_during_turn("s1"));

        ts.activate("s1", "shell");
        assert!(ts.activated_during_turn("s1"));

        ts.reset_turn_flag("s1");
        assert!(!ts.activated_during_turn("s1"));
    }

    #[test]
    fn has_inactive_tools() {
        let ts = ToolSet::new(&["read"], &["read", "shell", "web"]);
        assert!(ts.has_inactive_tools("s1"));

        ts.activate("s1", "shell");
        assert!(ts.has_inactive_tools("s1")); // web still inactive

        ts.activate("s1", "web");
        assert!(!ts.has_inactive_tools("s1")); // all active
    }

    #[test]
    fn is_known() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        assert!(ts.is_known("read"));
        assert!(ts.is_known("shell"));
        assert!(!ts.is_known("nope"));
    }

    #[test]
    fn cleanup() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        ts.activate("s1", "shell");
        assert!(ts.is_active("s1", "shell"));

        ts.cleanup("s1");
        assert!(!ts.is_active("s1", "shell"));
        assert!(!ts.activated_during_turn("s1"));
    }

    #[test]
    fn core_is_always_active() {
        let ts = ToolSet::new(&["read"], &["read", "shell"]);
        assert!(ts.is_active("any_session", "read"));
    }

    #[test]
    fn concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let ts = Arc::new(ToolSet::new(
            &["read"],
            &["read", "shell", "web", "fetch"],
        ));

        let mut handles = Vec::new();
        for i in 0..100 {
            let ts = Arc::clone(&ts);
            handles.push(thread::spawn(move || {
                let sid = format!("s{}", i % 10);
                ts.activate(&sid, "shell");
                ts.active_tool_names(&sid);
                ts.is_active(&sid, "shell");
                ts.activated_during_turn(&sid);
                ts.reset_turn_flag(&sid);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
