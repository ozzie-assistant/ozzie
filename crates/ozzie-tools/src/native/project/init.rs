use std::path::{Path, PathBuf};
use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_core::project::{load_project, ProjectRegistry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Initializes a new project workspace with `.ozzie/project.yaml` and `git init`.
pub struct InitProjectTool {
    registry: Arc<ProjectRegistry>,
    workspaces_root: PathBuf,
}

impl InitProjectTool {
    pub fn new(registry: Arc<ProjectRegistry>, workspaces_root: PathBuf) -> Self {
        Self {
            registry,
            workspaces_root,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "init_project".to_string(),
            description:
                "Initialize a new project workspace. Creates .ozzie/project.yaml, .ozzie/ozzie.md, and initializes a git repository."
                    .to_string(),
            parameters: schema_for::<InitProjectInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct InitProjectInput {
    /// Project name (used as directory name under workspaces_root).
    name: String,
    /// Short description of the project.
    description: String,
    /// Tags for categorization.
    #[serde(default)]
    tags: Vec<String>,
    /// Skills to link to this project.
    #[serde(default)]
    skills: Vec<String>,
    /// Enable automatic git commits after file writes.
    #[serde(default)]
    git_auto_commit: bool,
}

#[derive(Serialize)]
struct InitProjectOutput {
    name: String,
    path: String,
    message: String,
}

#[async_trait::async_trait]
impl Tool for InitProjectTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "init_project",
            "Initialize a new project workspace",
            InitProjectTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: InitProjectInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        // Validate name (safe for filesystem)
        if input.name.is_empty()
            || input.name.contains('/')
            || input.name.contains('\\')
            || input.name.starts_with('.')
        {
            return Err(ToolError::Execution(
                "invalid project name: must not be empty, start with '.', or contain path separators".to_string(),
            ));
        }

        // Check for duplicates
        if self.registry.get(&input.name).is_some() {
            return Err(ToolError::Execution(format!(
                "project '{}' already exists",
                input.name
            )));
        }

        let project_dir = self.workspaces_root.join(&input.name);
        let ozzie_dir = project_dir.join(".ozzie");

        // Create directories
        std::fs::create_dir_all(&ozzie_dir)
            .map_err(|e| ToolError::Execution(format!("create directory: {e}")))?;

        // Write project.yaml
        let yaml_content = build_project_yaml(&input);
        std::fs::write(ozzie_dir.join("project.yaml"), &yaml_content)
            .map_err(|e| ToolError::Execution(format!("write project.yaml: {e}")))?;

        // Write ozzie.md placeholder
        let md_path = ozzie_dir.join("ozzie.md");
        if !md_path.exists() {
            std::fs::write(
                &md_path,
                format!("# {}\n\n{}\n", input.name, input.description),
            )
            .map_err(|e| ToolError::Execution(format!("write ozzie.md: {e}")))?;
        }

        // Git init (only if not already a git repo)
        if !project_dir.join(".git").exists() {
            git_init(&project_dir)?;
        }

        // Load and register the new project
        match load_project(&project_dir) {
            Ok(manifest) => {
                self.registry.register(manifest);
            }
            Err(e) => {
                tracing::warn!(error = %e, "project created but failed to register");
            }
        }

        let output = InitProjectOutput {
            name: input.name,
            path: project_dir.to_string_lossy().to_string(),
            message: "Project initialized successfully.".to_string(),
        };

        serde_json::to_string_pretty(&output)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}

fn build_project_yaml(input: &InitProjectInput) -> String {
    let mut yaml = format!("name: {}\ndescription: {}\n", input.name, input.description);

    if !input.tags.is_empty() {
        yaml.push_str("tags:\n");
        for tag in &input.tags {
            yaml.push_str(&format!("  - {tag}\n"));
        }
    }

    if !input.skills.is_empty() {
        yaml.push_str("skills:\n");
        for skill in &input.skills {
            yaml.push_str(&format!("  - {skill}\n"));
        }
    }

    if input.git_auto_commit {
        yaml.push_str("git_auto_commit: true\n");
    }

    yaml
}

fn git_init(dir: &Path) -> Result<(), ToolError> {
    let output = std::process::Command::new("git")
        .arg("init")
        .current_dir(dir)
        .output()
        .map_err(|e| ToolError::Execution(format!("git init: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolError::Execution(format!("git init failed: {stderr}")));
    }

    Ok(())
}
