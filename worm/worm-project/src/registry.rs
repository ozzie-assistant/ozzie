use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::ProjectManifest;

/// Manages discovered projects by name.
pub struct ProjectRegistry {
    projects: RwLock<HashMap<String, ProjectManifest>>,
}

impl ProjectRegistry {
    pub fn new() -> Self {
        Self {
            projects: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a project (overwrites if name already exists).
    pub fn register(&self, project: ProjectManifest) {
        let mut projects = self.projects.write().unwrap();
        projects.insert(project.name.clone(), project);
    }

    /// Removes a project by name.
    pub fn unregister(&self, name: &str) -> Option<ProjectManifest> {
        let mut projects = self.projects.write().unwrap();
        projects.remove(name)
    }

    /// Looks up a project by name.
    pub fn get(&self, name: &str) -> Option<ProjectManifest> {
        let projects = self.projects.read().unwrap();
        projects.get(name).cloned()
    }

    /// Returns all projects sorted by name.
    pub fn all(&self) -> Vec<ProjectManifest> {
        let projects = self.projects.read().unwrap();
        let mut all: Vec<ProjectManifest> = projects.values().cloned().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// Returns a name → description map for UI/tool display.
    pub fn catalog(&self) -> HashMap<String, String> {
        let projects = self.projects.read().unwrap();
        projects
            .iter()
            .map(|(name, p)| (name.clone(), p.description.clone()))
            .collect()
    }

    /// Returns the number of registered projects.
    pub fn len(&self) -> usize {
        let projects = self.projects.read().unwrap();
        projects.len()
    }

    /// Returns true if no projects are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(name: &str) -> ProjectManifest {
        ProjectManifest {
            name: name.to_string(),
            description: format!("{name} project"),
            skills: Vec::new(),
            tags: Vec::new(),
            git_auto_commit: false,
            memory: None,
            instructions: String::new(),
            path: format!("/tmp/{name}"),
        }
    }

    #[test]
    fn register_and_get() {
        let reg = ProjectRegistry::new();
        reg.register(make_project("coaching"));

        let project = reg.get("coaching").unwrap();
        assert_eq!(project.name, "coaching");
        assert_eq!(project.path, "/tmp/coaching");
    }

    #[test]
    fn unregister() {
        let reg = ProjectRegistry::new();
        reg.register(make_project("coaching"));

        let removed = reg.unregister("coaching");
        assert!(removed.is_some());
        assert!(reg.get("coaching").is_none());
        assert!(reg.is_empty());
    }

    #[test]
    fn catalog() {
        let reg = ProjectRegistry::new();
        reg.register(make_project("coaching"));
        reg.register(make_project("email"));

        let catalog = reg.catalog();
        assert_eq!(catalog.len(), 2);
        assert!(catalog.contains_key("coaching"));
        assert!(catalog.contains_key("email"));
    }

    #[test]
    fn all_sorted() {
        let reg = ProjectRegistry::new();
        reg.register(make_project("zebra"));
        reg.register(make_project("alpha"));

        let all = reg.all();
        assert_eq!(all[0].name, "alpha");
        assert_eq!(all[1].name, "zebra");
    }
}
