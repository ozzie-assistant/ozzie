use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::loader::{self, SkillLoadError};
use crate::types::SkillMD;

/// Port for skill loading.
#[async_trait::async_trait]
pub trait SkillRepository: Send + Sync {
    /// Loads all available skills.
    async fn load_all(&self) -> Vec<SkillMD>;

    /// Loads a single skill from its SKILL.md path.
    async fn load_one(&self, path: &Path) -> Result<SkillMD, SkillLoadError>;
}

/// File-based skill repository.
///
/// Scans `<skills_dir>/*/SKILL.md` for skill definitions.
pub struct FsSkillRepository {
    skills_dir: PathBuf,
}

impl FsSkillRepository {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
        }
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }
}

#[async_trait::async_trait]
impl SkillRepository for FsSkillRepository {
    async fn load_all(&self) -> Vec<SkillMD> {
        loader::load_skills_dir(&self.skills_dir).await
    }

    async fn load_one(&self, path: &Path) -> Result<SkillMD, SkillLoadError> {
        loader::parse_skill_md(path).await
    }
}

/// In-memory skill repository for testing.
pub struct InMemorySkillRepository {
    skills: RwLock<Vec<SkillMD>>,
}

impl InMemorySkillRepository {
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(Vec::new()),
        }
    }

    pub fn with_skills(skills: Vec<SkillMD>) -> Self {
        Self {
            skills: RwLock::new(skills),
        }
    }

    pub fn add(&self, skill: SkillMD) {
        let mut skills = self.skills.write().unwrap();
        skills.push(skill);
    }
}

impl Default for InMemorySkillRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SkillRepository for InMemorySkillRepository {
    async fn load_all(&self) -> Vec<SkillMD> {
        let skills = self.skills.read().unwrap();
        skills.clone()
    }

    async fn load_one(&self, _path: &Path) -> Result<SkillMD, SkillLoadError> {
        Err(SkillLoadError::Io(
            "in-memory repository does not support load_one by path".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_skill(name: &str) -> SkillMD {
        SkillMD {
            name: name.to_string(),
            description: format!("{name} description"),
            license: None,
            compatibility: None,
            metadata: HashMap::new(),
            allowed_tools: Vec::new(),
            body: String::new(),
            dir: name.to_string(),
            workflow: None,
            triggers: None,
            source: Default::default(),
        }
    }

    #[tokio::test]
    async fn fs_load_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FsSkillRepository::new(dir.path());
        let skills = repo.load_all().await;
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn fs_load_skill() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("deploy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploy\n---\nBody",
        )
        .unwrap();

        let repo = FsSkillRepository::new(dir.path());
        let skills = repo.load_all().await;
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deploy");
    }

    #[tokio::test]
    async fn in_memory_empty() {
        let repo = InMemorySkillRepository::new();
        assert!(repo.load_all().await.is_empty());
    }

    #[tokio::test]
    async fn in_memory_with_skills() {
        let repo = InMemorySkillRepository::with_skills(vec![
            test_skill("a"),
            test_skill("b"),
        ]);
        let skills = repo.load_all().await;
        assert_eq!(skills.len(), 2);
    }

    #[tokio::test]
    async fn in_memory_add() {
        let repo = InMemorySkillRepository::new();
        repo.add(test_skill("x"));
        assert_eq!(repo.load_all().await.len(), 1);
    }
}
