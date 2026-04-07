use std::collections::HashMap;

use super::types::Policy;

/// Resolves a policy by name, merging config overrides with defaults.
pub struct PolicyResolver {
    policies: HashMap<String, Policy>,
}

impl PolicyResolver {
    /// Creates a resolver with predefined policies, then applies overrides.
    pub fn new(overrides: HashMap<String, PolicyOverride>) -> Self {
        let mut policies: HashMap<String, Policy> = Policy::predefined()
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();

        for (name, ov) in overrides {
            if let Some(base) = policies.get_mut(&name) {
                if let Some(skills) = ov.allowed_skills {
                    base.allowed_skills = Some(skills);
                }
                if let Some(tools) = ov.allowed_tools {
                    base.allowed_tools = Some(tools);
                }
                if !ov.denied_tools.is_empty() {
                    base.denied_tools = ov.denied_tools;
                }
                if let Some(mode) = ov.approval_mode {
                    base.approval_mode = mode;
                }
                if let Some(cf) = ov.client_facing {
                    base.client_facing = cf;
                }
                if let Some(mc) = ov.max_concurrent {
                    base.max_concurrent = mc;
                }
            }
        }

        Self { policies }
    }

    /// Returns the policy for the given name.
    pub fn resolve(&self, name: &str) -> Option<&Policy> {
        self.policies.get(name)
    }

    /// Returns all known policy names in sorted order.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.policies.keys().cloned().collect();
        names.sort();
        names
    }
}

/// Config override for a predefined policy.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PolicyOverride {
    #[serde(default)]
    pub allowed_skills: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub denied_tools: Vec<String>,
    #[serde(default)]
    pub approval_mode: Option<String>,
    #[serde(default)]
    pub client_facing: Option<bool>,
    #[serde(default)]
    pub max_concurrent: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_all_predefined() {
        let resolver = PolicyResolver::new(HashMap::new());
        let names = resolver.names();
        assert!(names.contains(&"admin".to_string()));
        assert!(names.contains(&"support".to_string()));
        assert!(names.contains(&"executor".to_string()));
        assert!(names.contains(&"readonly".to_string()));
    }

    #[test]
    fn override_applies() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "admin".to_string(),
            PolicyOverride {
                max_concurrent: Some(8),
                ..Default::default()
            },
        );

        let resolver = PolicyResolver::new(overrides);
        let admin = resolver.resolve("admin").unwrap();
        assert_eq!(admin.max_concurrent, 8);
    }

    #[test]
    fn unknown_override_ignored() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "unknown_policy".to_string(),
            PolicyOverride::default(),
        );

        let resolver = PolicyResolver::new(overrides);
        assert!(resolver.resolve("unknown_policy").is_none());
    }
}
