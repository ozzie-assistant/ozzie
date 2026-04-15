use std::path::{Path, PathBuf};

use tracing::{debug, warn};

use super::types::ProjectManifest;

const PROJECT_YAML: &str = ".ozzie/project.yaml";
const OZZIE_MD: &str = ".ozzie/ozzie.md";

/// Discovers projects under `workspaces_root` by scanning `*/.ozzie/project.yaml`.
pub fn discover_projects(workspaces_root: &Path, extra_paths: &[String]) -> Vec<ProjectManifest> {
    let mut projects = Vec::new();

    // Scan workspaces_root children
    if workspaces_root.exists() {
        match std::fs::read_dir(workspaces_root) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        try_load_project(&path, &mut projects);
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, path = %workspaces_root.display(), "failed to read workspaces root");
            }
        }
    } else {
        debug!(path = %workspaces_root.display(), "workspaces root not found");
    }

    // Scan extra paths
    for extra in extra_paths {
        let path = expand_tilde(extra);
        if path.is_dir() {
            try_load_project(&path, &mut projects);
        } else {
            warn!(path = %path.display(), "extra project path not found");
        }
    }

    projects.sort_by(|a, b| a.name.cmp(&b.name));
    debug!(count = projects.len(), "projects discovered");
    projects
}

fn try_load_project(dir: &Path, projects: &mut Vec<ProjectManifest>) {
    let yaml_file = dir.join(PROJECT_YAML);
    if !yaml_file.exists() {
        return;
    }
    match load_project(dir) {
        Ok(manifest) => {
            debug!(name = %manifest.name, path = %dir.display(), "loaded project");
            projects.push(manifest);
        }
        Err(e) => {
            warn!(path = %yaml_file.display(), error = %e, "failed to parse project");
        }
    }
}

/// Loads a project from a directory containing `.ozzie/project.yaml`.
pub fn load_project(project_root: &Path) -> Result<ProjectManifest, ProjectLoadError> {
    let yaml_path = project_root.join(PROJECT_YAML);
    let yaml_content = std::fs::read_to_string(&yaml_path)
        .map_err(|e| ProjectLoadError::Io(e.to_string()))?;

    let mut manifest: ProjectManifest =
        serde_yaml::from_str(&yaml_content).map_err(|e| ProjectLoadError::Parse(e.to_string()))?;

    // Resolve absolute path
    manifest.path = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();

    // Default name to directory name if empty
    if manifest.name.is_empty() {
        manifest.name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    // Load instructions from ozzie.md (optional)
    let md_path = project_root.join(OZZIE_MD);
    if md_path.exists() {
        manifest.instructions = std::fs::read_to_string(&md_path)
            .map_err(|e| ProjectLoadError::Io(e.to_string()))?;
    }

    Ok(manifest)
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectLoadError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

/// Expands `~` prefix to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let projects = discover_projects(dir.path(), &[]);
        assert!(projects.is_empty());
    }

    #[test]
    fn discover_nonexistent_dir() {
        let projects = discover_projects(Path::new("/nonexistent"), &[]);
        assert!(projects.is_empty());
    }

    #[test]
    fn load_project_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let ozzie_dir = dir.path().join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();

        let yaml = r#"
name: coaching
description: Programme musculation
skills:
  - log-session
tags:
  - sport
git_auto_commit: true
memory:
  scan_cron: "0 22 * * *"
  patterns:
    - "**/*.md"
    - "!_index.md"
  max_file_chars: 8000
  extract:
    - type: fact
      focus: "métriques de performance"
    - type: procedure
      focus: "ajustements de programme"
"#;
        std::fs::write(ozzie_dir.join("project.yaml"), yaml).unwrap();
        std::fs::write(ozzie_dir.join("ozzie.md"), "# Coaching\n\nObjectif: squat 120kg.").unwrap();

        let manifest = load_project(dir.path()).unwrap();
        assert_eq!(manifest.name, "coaching");
        assert_eq!(manifest.description, "Programme musculation");
        assert_eq!(manifest.skills, vec!["log-session"]);
        assert_eq!(manifest.tags, vec!["sport"]);
        assert!(manifest.git_auto_commit);
        assert_eq!(manifest.instructions, "# Coaching\n\nObjectif: squat 120kg.");

        let mem = manifest.memory.unwrap();
        assert_eq!(mem.scan_cron.as_deref(), Some("0 22 * * *"));
        assert_eq!(mem.patterns, vec!["**/*.md", "!_index.md"]);
        assert_eq!(mem.max_file_chars, 8000);
        assert_eq!(mem.extract.len(), 2);
        assert_eq!(mem.extract[0].memory_type, "fact");
        assert_eq!(mem.extract[0].focus, "métriques de performance");
    }

    #[test]
    fn load_minimal_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let ozzie_dir = dir.path().join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();
        std::fs::write(ozzie_dir.join("project.yaml"), "name: minimal\n").unwrap();

        let manifest = load_project(dir.path()).unwrap();
        assert_eq!(manifest.name, "minimal");
        assert!(manifest.description.is_empty());
        assert!(manifest.memory.is_none());
        assert!(manifest.instructions.is_empty());
    }

    #[test]
    fn load_without_ozzie_md() {
        let dir = tempfile::tempdir().unwrap();
        let ozzie_dir = dir.path().join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();
        std::fs::write(ozzie_dir.join("project.yaml"), "name: bare\ndescription: No instructions\n").unwrap();

        let manifest = load_project(dir.path()).unwrap();
        assert_eq!(manifest.name, "bare");
        assert!(manifest.instructions.is_empty());
    }

    #[test]
    fn discover_project_in_workspaces_root() {
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("coaching");
        let ozzie_dir = project_dir.join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();
        std::fs::write(
            ozzie_dir.join("project.yaml"),
            "name: coaching\ndescription: Sport\n",
        )
        .unwrap();

        let projects = discover_projects(root.path(), &[]);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "coaching");
    }

    #[test]
    fn discover_extra_path() {
        let extra = tempfile::tempdir().unwrap();
        let ozzie_dir = extra.path().join(".ozzie");
        std::fs::create_dir_all(&ozzie_dir).unwrap();
        std::fs::write(
            ozzie_dir.join("project.yaml"),
            "name: external\ndescription: External project\n",
        )
        .unwrap();

        let empty_root = tempfile::tempdir().unwrap();
        let projects = discover_projects(
            empty_root.path(),
            &[extra.path().to_string_lossy().to_string()],
        );
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "external");
    }
}
