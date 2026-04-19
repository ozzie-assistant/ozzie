use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::{SkillMD, SkillSource};

/// Manages loaded skills by name.
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, SkillMD>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a skill.
    pub fn register(&self, skill: SkillMD) {
        let mut skills = self.skills.write().unwrap();
        skills.insert(skill.name.clone(), skill);
    }

    /// Looks up a skill by name.
    pub fn get(&self, name: &str) -> Option<SkillMD> {
        let skills = self.skills.read().unwrap();
        skills.get(name).cloned()
    }

    /// Returns all skills sorted by name.
    pub fn all(&self) -> Vec<SkillMD> {
        let skills = self.skills.read().unwrap();
        let mut all: Vec<SkillMD> = skills.values().cloned().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// Returns a name → description map for UI display.
    pub fn catalog(&self) -> HashMap<String, String> {
        let skills = self.skills.read().unwrap();
        skills
            .iter()
            .map(|(name, skill)| (name.clone(), skill.description.clone()))
            .collect()
    }

    /// Returns the number of registered skills.
    pub fn len(&self) -> usize {
        let skills = self.skills.read().unwrap();
        skills.len()
    }

    /// Returns true if no skills are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes a skill by name. Returns the removed skill if it existed.
    pub fn unregister(&self, name: &str) -> Option<SkillMD> {
        let mut skills = self.skills.write().unwrap();
        skills.remove(name)
    }

    /// Removes all skills matching a given source. Returns the count removed.
    pub fn unregister_by_source(&self, source: &SkillSource) -> usize {
        let mut skills = self.skills.write().unwrap();
        let before = skills.len();
        skills.retain(|_, skill| &skill.source != source);
        before - skills.len()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str) -> SkillMD {
        SkillMD {
            name: name.to_string(),
            description: format!("{name} skill"),
            license: None,
            compatibility: None,
            metadata: HashMap::new(),
            allowed_tools: Vec::new(),
            body: String::new(),
            dir: String::new(),
            workflow: None,
            triggers: None,
            source: SkillSource::Global,
        }
    }

    #[test]
    fn register_and_get() {
        let reg = SkillRegistry::new();
        reg.register(make_skill("deploy"));

        let skill = reg.get("deploy").unwrap();
        assert_eq!(skill.name, "deploy");
    }

    #[test]
    fn catalog() {
        let reg = SkillRegistry::new();
        reg.register(make_skill("deploy"));
        reg.register(make_skill("backup"));

        let catalog = reg.catalog();
        assert_eq!(catalog.len(), 2);
        assert!(catalog.contains_key("deploy"));
    }

    #[test]
    fn all_sorted() {
        let reg = SkillRegistry::new();
        reg.register(make_skill("zebra"));
        reg.register(make_skill("alpha"));

        let all = reg.all();
        assert_eq!(all[0].name, "alpha");
        assert_eq!(all[1].name, "zebra");
    }

    #[test]
    fn unregister() {
        let reg = SkillRegistry::new();
        reg.register(make_skill("deploy"));

        let removed = reg.unregister("deploy");
        assert!(removed.is_some());
        assert!(reg.get("deploy").is_none());
    }

    #[test]
    fn unregister_by_source() {
        let reg = SkillRegistry::new();
        reg.register(make_skill("global-a"));

        let mut project_skill = make_skill("project-b");
        project_skill.source = SkillSource::Project("coaching".to_string());
        reg.register(project_skill);

        let mut project_skill2 = make_skill("project-c");
        project_skill2.source = SkillSource::Project("coaching".to_string());
        reg.register(project_skill2);

        assert_eq!(reg.len(), 3);

        let removed = reg.unregister_by_source(&SkillSource::Project("coaching".to_string()));
        assert_eq!(removed, 2);
        assert_eq!(reg.len(), 1);
        assert!(reg.get("global-a").is_some());
        assert!(reg.get("project-b").is_none());
    }
}
