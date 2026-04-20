use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::loader::{self, ProjectLoadError};
use crate::types::ProjectManifest;

/// Port for project discovery and loading.
#[async_trait::async_trait]
pub trait ProjectRepository: Send + Sync {
    /// Discovers all available projects.
    async fn discover(&self) -> Vec<ProjectManifest>;

    /// Loads a single project from its root directory.
    async fn load(&self, project_root: &Path) -> Result<ProjectManifest, ProjectLoadError>;
}

/// File-based project repository.
///
/// Scans `workspaces_root` children and `extra_paths` for `.ozzie/project.yaml`.
pub struct FsProjectRepository {
    workspaces_root: PathBuf,
    extra_paths: Vec<String>,
}

impl FsProjectRepository {
    pub fn new(workspaces_root: impl Into<PathBuf>, extra_paths: Vec<String>) -> Self {
        Self {
            workspaces_root: workspaces_root.into(),
            extra_paths,
        }
    }

    pub fn workspaces_root(&self) -> &Path {
        &self.workspaces_root
    }
}

#[async_trait::async_trait]
impl ProjectRepository for FsProjectRepository {
    async fn discover(&self) -> Vec<ProjectManifest> {
        loader::discover_projects(&self.workspaces_root, &self.extra_paths).await
    }

    async fn load(&self, project_root: &Path) -> Result<ProjectManifest, ProjectLoadError> {
        loader::load_project(project_root).await
    }
}

/// In-memory project repository for testing.
pub struct InMemoryProjectRepository {
    projects: RwLock<Vec<ProjectManifest>>,
}

impl InMemoryProjectRepository {
    pub fn new() -> Self {
        Self {
            projects: RwLock::new(Vec::new()),
        }
    }

    pub fn with_projects(projects: Vec<ProjectManifest>) -> Self {
        Self {
            projects: RwLock::new(projects),
        }
    }

    pub fn add(&self, project: ProjectManifest) {
        let mut projects = self.projects.write().unwrap();
        projects.push(project);
    }
}

impl Default for InMemoryProjectRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ProjectRepository for InMemoryProjectRepository {
    async fn discover(&self) -> Vec<ProjectManifest> {
        let projects = self.projects.read().unwrap();
        projects.clone()
    }

    async fn load(&self, _project_root: &Path) -> Result<ProjectManifest, ProjectLoadError> {
        Err(ProjectLoadError::Io(
            "in-memory repository does not support load by path".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest(name: &str) -> ProjectManifest {
        ProjectManifest {
            name: name.to_string(),
            description: format!("{name} project"),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn fs_discover_empty() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FsProjectRepository::new(dir.path(), vec![]);
        let projects = repo.discover().await;
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn fs_discover_finds_project() {
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("my-project");
        let ozzie_dir = project_dir.join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();
        std::fs::write(
            ozzie_dir.join("project.yaml"),
            "name: my-project\ndescription: Test\n",
        )
        .unwrap();

        let repo = FsProjectRepository::new(root.path(), vec![]);
        let projects = repo.discover().await;
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-project");
    }

    #[tokio::test]
    async fn in_memory_empty() {
        let repo = InMemoryProjectRepository::new();
        assert!(repo.discover().await.is_empty());
    }

    #[tokio::test]
    async fn in_memory_with_projects() {
        let repo = InMemoryProjectRepository::with_projects(vec![
            test_manifest("a"),
            test_manifest("b"),
        ]);
        assert_eq!(repo.discover().await.len(), 2);
    }

    #[tokio::test]
    async fn in_memory_add() {
        let repo = InMemoryProjectRepository::new();
        repo.add(test_manifest("x"));
        assert_eq!(repo.discover().await.len(), 1);
    }
}
