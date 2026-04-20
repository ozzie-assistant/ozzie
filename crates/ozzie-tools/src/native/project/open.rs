use std::sync::Arc;

use ozzie_core::domain::{SessionStore, Tool, ToolError, ToolInfo, TOOL_CTX};
use ozzie_core::project::ProjectRegistry;
use ozzie_core::skills::{FsSkillRepository, SkillRegistry, SkillRepository};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Opens a project: sets session's project_id, root_dir, and activates project skills.
pub struct OpenProjectTool {
    project_registry: Arc<ProjectRegistry>,
    skill_registry: Arc<SkillRegistry>,
    session_store: Arc<dyn SessionStore>,
}

impl OpenProjectTool {
    pub fn new(
        project_registry: Arc<ProjectRegistry>,
        skill_registry: Arc<SkillRegistry>,
        session_store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            project_registry,
            skill_registry,
            session_store,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "open_project".to_string(),
            description:
                "Open a project for this session. Sets the working directory, activates project-specific skills, and binds the session to the project."
                    .to_string(),
            parameters: schema_for::<OpenProjectInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct OpenProjectInput {
    /// Name of the project to open (as shown by list_projects).
    name: String,
}

#[derive(Serialize)]
struct OpenProjectOutput {
    name: String,
    path: String,
    skills_loaded: Vec<String>,
    message: String,
}

#[async_trait::async_trait]
impl Tool for OpenProjectTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "open_project",
            "Open a project for this session",
            OpenProjectTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: OpenProjectInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let project = self
            .project_registry
            .get(&input.name)
            .ok_or_else(|| ToolError::Execution(format!("project not found: {}", input.name)))?;

        // Update session: set project_id and root_dir
        let session_id = TOOL_CTX
            .try_with(|ctx| ctx.session_id.clone())
            .unwrap_or_default();

        if session_id.is_empty() {
            return Err(ToolError::Execution("no session in context".to_string()));
        }

        let mut session = self
            .session_store
            .get(&session_id)
            .await
            .map_err(|e| ToolError::Execution(format!("get session: {e}")))?
            .ok_or_else(|| ToolError::Execution(format!("session not found: {session_id}")))?;

        session.project_id = Some(project.name.clone());
        session.root_dir = Some(project.path.clone());
        session.updated_at = chrono::Utc::now();

        self.session_store
            .update(&session)
            .await
            .map_err(|e| ToolError::Execution(format!("update session: {e}")))?;

        // Load project-local skills from .ozzie/skills/
        let mut skills_loaded = Vec::new();
        let skills_dir = std::path::Path::new(&project.path).join(".ozzie/skills");
        if skills_dir.exists() {
            let local_skills = FsSkillRepository::new(&skills_dir).load_all().await;
            for mut skill in local_skills {
                skill.source =
                    ozzie_core::skills::SkillSource::Project(project.name.clone());
                skills_loaded.push(skill.name.clone());
                self.skill_registry.register(skill);
            }
        }

        let output = OpenProjectOutput {
            name: project.name.clone(),
            path: project.path.clone(),
            skills_loaded,
            message: format!(
                "Project '{}' opened. Working directory set to {}.",
                project.name, project.path
            ),
        };

        serde_json::to_string_pretty(&output)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}
