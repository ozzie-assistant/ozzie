use serde::{Deserialize, Serialize};

/// Defines what a session can do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub name: String,
    /// "persistent" | "ephemeral" | "per-request"
    #[serde(default)]
    pub session_mode: String,
    /// Allowed skill names (None = all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_skills: Option<Vec<String>>,
    /// Allowed tool names (None = all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Denied tool names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_tools: Vec<String>,
    /// "sync" | "async" | "none"
    #[serde(default)]
    pub approval_mode: String,
    /// Inject Persona for client-facing sessions.
    #[serde(default)]
    pub client_facing: bool,
    #[serde(default)]
    pub max_concurrent: usize,
}

impl Policy {
    pub fn admin() -> Self {
        Self {
            name: "admin".to_string(),
            session_mode: "persistent".to_string(),
            allowed_skills: None,
            allowed_tools: None,
            denied_tools: Vec::new(),
            approval_mode: "sync".to_string(),
            client_facing: true,
            max_concurrent: 4,
        }
    }

    pub fn support() -> Self {
        Self {
            name: "support".to_string(),
            session_mode: "ephemeral".to_string(),
            allowed_skills: None,
            allowed_tools: None,
            denied_tools: vec![
                "run_command".to_string(),
                "write_file".to_string(),
                "edit_file".to_string(),
            ],
            approval_mode: "none".to_string(),
            client_facing: true,
            max_concurrent: 2,
        }
    }

    pub fn executor() -> Self {
        Self {
            name: "executor".to_string(),
            session_mode: "per-request".to_string(),
            allowed_skills: None,
            allowed_tools: None,
            denied_tools: Vec::new(),
            approval_mode: "none".to_string(),
            client_facing: false,
            max_concurrent: 2,
        }
    }

    pub fn readonly() -> Self {
        Self {
            name: "readonly".to_string(),
            session_mode: "ephemeral".to_string(),
            allowed_skills: None,
            allowed_tools: Some(Vec::new()),
            denied_tools: Vec::new(),
            approval_mode: "none".to_string(),
            client_facing: true,
            max_concurrent: 1,
        }
    }

    /// Returns all predefined policies.
    pub fn predefined() -> Vec<Self> {
        vec![Self::admin(), Self::support(), Self::executor(), Self::readonly()]
    }

    /// Looks up a predefined policy by name.
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "admin" => Some(Self::admin()),
            "support" => Some(Self::support()),
            "executor" => Some(Self::executor()),
            "readonly" => Some(Self::readonly()),
            _ => None,
        }
    }

    /// Returns `true` if this policy allows the given tool to be used.
    ///
    /// Logic: tool is allowed if it is NOT in `denied_tools` AND either
    /// `allowed_tools` is `None` (all allowed) or it appears in `allowed_tools`.
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        if self.denied_tools.iter().any(|d| d == tool_name) {
            return false;
        }
        match &self.allowed_tools {
            None => true,
            Some(allowed) => allowed.iter().any(|a| a == tool_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predefined_policies() {
        let policies = Policy::predefined();
        assert_eq!(policies.len(), 4);
        assert!(policies.iter().any(|p| p.name == "admin"));
        assert!(policies.iter().any(|p| p.name == "readonly"));
    }

    #[test]
    fn admin_allows_all() {
        let p = Policy::admin();
        assert!(p.allowed_tools.is_none());
        assert!(p.allowed_skills.is_none());
        assert!(p.client_facing);
    }

    #[test]
    fn readonly_allows_no_tools() {
        let p = Policy::readonly();
        assert_eq!(p.allowed_tools, Some(Vec::new()));
    }
}
