use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Thread-safe store for tool approval state.
///
/// Tracks which tools are approved globally (from config) and per-session
/// (from user approval flow).
pub struct ToolPermissions {
    /// Globally allowed tools (from config).
    global_allowed: HashSet<String>,
    /// Per-session approvals: session_id → set of tool names. "*" means all.
    session_allowed: RwLock<HashMap<String, HashSet<String>>>,
}

impl ToolPermissions {
    pub fn new(global_allowed: Vec<String>) -> Self {
        Self {
            global_allowed: global_allowed.into_iter().collect(),
            session_allowed: RwLock::new(HashMap::new()),
        }
    }

    /// Returns true if the tool is approved for this session.
    pub fn is_allowed(&self, session_id: &str, tool_name: &str) -> bool {
        if self.global_allowed.contains(tool_name) {
            return true;
        }
        let sessions = self.session_allowed.read().unwrap();
        if let Some(tools) = sessions.get(session_id) {
            tools.contains("*") || tools.contains(tool_name)
        } else {
            false
        }
    }

    /// Approves a single tool for a session.
    pub fn allow_for_session(&self, session_id: &str, tool_name: &str) {
        let mut sessions = self.session_allowed.write().unwrap();
        sessions
            .entry(session_id.to_string())
            .or_default()
            .insert(tool_name.to_string());
    }

    /// Enables accept-all mode for a session.
    pub fn allow_all_for_session(&self, session_id: &str) {
        let mut sessions = self.session_allowed.write().unwrap();
        sessions
            .entry(session_id.to_string())
            .or_default()
            .insert("*".to_string());
    }

    /// Returns true if the session has accept-all mode enabled.
    pub fn is_session_accept_all(&self, session_id: &str) -> bool {
        let sessions = self.session_allowed.read().unwrap();
        sessions
            .get(session_id)
            .is_some_and(|tools| tools.contains("*"))
    }

    /// Removes all approvals for a session.
    pub fn cleanup_session(&self, session_id: &str) {
        let mut sessions = self.session_allowed.write().unwrap();
        sessions.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_allowed() {
        let perms = ToolPermissions::new(vec!["safe_tool".to_string()]);
        assert!(perms.is_allowed("s1", "safe_tool"));
        assert!(!perms.is_allowed("s1", "dangerous_tool"));
    }

    #[test]
    fn session_approval() {
        let perms = ToolPermissions::new(vec![]);
        assert!(!perms.is_allowed("s1", "cmd"));

        perms.allow_for_session("s1", "cmd");
        assert!(perms.is_allowed("s1", "cmd"));
        assert!(!perms.is_allowed("s2", "cmd"));
    }

    #[test]
    fn session_accept_all() {
        let perms = ToolPermissions::new(vec![]);
        perms.allow_all_for_session("s1");

        assert!(perms.is_allowed("s1", "anything"));
        assert!(perms.is_session_accept_all("s1"));
        assert!(!perms.is_session_accept_all("s2"));
    }

    #[test]
    fn cleanup_session() {
        let perms = ToolPermissions::new(vec![]);
        perms.allow_for_session("s1", "cmd");
        assert!(perms.is_allowed("s1", "cmd"));

        perms.cleanup_session("s1");
        assert!(!perms.is_allowed("s1", "cmd"));
    }
}
