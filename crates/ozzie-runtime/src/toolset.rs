use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Two-tier tool management: core (always active) + plugin (activate on demand).
///
/// Tracks per-session active/inactive tools and detects mid-turn activations.
pub struct TwoTierToolSet {
    /// Tools that are always active for every session.
    core_tools: HashSet<String>,
    /// All known tool names (core + plugin).
    all_tools: HashSet<String>,
    /// Per-session activated plugin tools.
    conversation_active: RwLock<HashMap<String, ConversationToolState>>,
}

struct ConversationToolState {
    active: HashSet<String>,
    activated_this_turn: bool,
}

impl TwoTierToolSet {
    pub fn new(core_tools: Vec<String>, all_tools: Vec<String>) -> Self {
        let core: HashSet<String> = core_tools.into_iter().collect();
        let all: HashSet<String> = all_tools.into_iter().collect();
        Self {
            core_tools: core,
            all_tools: all,
            conversation_active: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the names of tools currently active for a session.
    pub fn active_tool_names(&self, conversation_id: &str) -> Vec<String> {
        let sessions = self.conversation_active.read().unwrap();
        let mut names: Vec<String> = self.core_tools.iter().cloned().collect();
        if let Some(state) = sessions.get(conversation_id) {
            names.extend(state.active.iter().cloned());
        }
        names.sort();
        names.dedup();
        names
    }

    /// Returns tools that exist but are not active for this session.
    pub fn inactive_tool_names(&self, conversation_id: &str) -> Vec<String> {
        let active = self.active_tool_names(conversation_id);
        let active_set: HashSet<&String> = active.iter().collect();
        self.all_tools
            .iter()
            .filter(|t| !active_set.contains(t))
            .cloned()
            .collect()
    }

    /// Returns true if there are inactive tools for this session.
    pub fn has_inactive_tools(&self, conversation_id: &str) -> bool {
        !self.inactive_tool_names(conversation_id).is_empty()
    }

    /// Activates a tool for a session. Returns true if the tool was newly activated.
    pub fn activate(&self, conversation_id: &str, tool_name: &str) -> bool {
        if !self.all_tools.contains(tool_name) {
            return false;
        }
        if self.core_tools.contains(tool_name) {
            return false; // already always active
        }

        let mut sessions = self.conversation_active.write().unwrap();
        let state = sessions.entry(conversation_id.to_string()).or_insert_with(|| {
            ConversationToolState {
                active: HashSet::new(),
                activated_this_turn: false,
            }
        });

        let inserted = state.active.insert(tool_name.to_string());
        if inserted {
            state.activated_this_turn = true;
        }
        inserted
    }

    /// Returns true if a tool was activated during the current turn.
    pub fn activated_during_turn(&self, conversation_id: &str) -> bool {
        let sessions = self.conversation_active.read().unwrap();
        sessions
            .get(conversation_id)
            .is_some_and(|s| s.activated_this_turn)
    }

    /// Resets the turn-activation flag for a session.
    pub fn reset_turn_flag(&self, conversation_id: &str) {
        let mut sessions = self.conversation_active.write().unwrap();
        if let Some(state) = sessions.get_mut(conversation_id) {
            state.activated_this_turn = false;
        }
    }

    /// Returns true if the tool name is known (core or plugin).
    pub fn is_known(&self, tool_name: &str) -> bool {
        self.all_tools.contains(tool_name)
    }

    /// Cleans up session state.
    pub fn cleanup_session(&self, conversation_id: &str) {
        let mut sessions = self.conversation_active.write().unwrap();
        sessions.remove(conversation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_toolset() -> TwoTierToolSet {
        TwoTierToolSet::new(
            vec!["execute".to_string(), "file_read".to_string()],
            vec![
                "execute".to_string(),
                "file_read".to_string(),
                "web_fetch".to_string(),
                "git".to_string(),
            ],
        )
    }

    #[test]
    fn active_tools_starts_with_core() {
        let ts = make_toolset();
        let active = ts.active_tool_names("s1");
        assert!(active.contains(&"execute".to_string()));
        assert!(active.contains(&"file_read".to_string()));
        assert!(!active.contains(&"web_fetch".to_string()));
    }

    #[test]
    fn activate_plugin_tool() {
        let ts = make_toolset();
        assert!(ts.has_inactive_tools("s1"));

        let activated = ts.activate("s1", "web_fetch");
        assert!(activated);
        assert!(ts.active_tool_names("s1").contains(&"web_fetch".to_string()));
    }

    #[test]
    fn activate_unknown_tool_fails() {
        let ts = make_toolset();
        assert!(!ts.activate("s1", "unknown_tool"));
    }

    #[test]
    fn turn_flag_tracking() {
        let ts = make_toolset();
        assert!(!ts.activated_during_turn("s1"));

        ts.activate("s1", "git");
        assert!(ts.activated_during_turn("s1"));

        ts.reset_turn_flag("s1");
        assert!(!ts.activated_during_turn("s1"));
    }

    #[test]
    fn cleanup_session() {
        let ts = make_toolset();
        ts.activate("s1", "web_fetch");
        assert!(ts.active_tool_names("s1").contains(&"web_fetch".to_string()));

        ts.cleanup_session("s1");
        assert!(!ts.active_tool_names("s1").contains(&"web_fetch".to_string()));
    }
}
