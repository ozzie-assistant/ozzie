use std::sync::Arc;

use ozzie_core::domain::{SessionStore, Tool, ToolError, ToolInfo, TOOL_CTX};
use ozzie_core::skills::{SkillRegistry, SkillSource};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Closes the active project: clears session binding and unloads project skills.
pub struct CloseProjectTool {
    skill_registry: Arc<SkillRegistry>,
    session_store: Arc<dyn SessionStore>,
}

impl CloseProjectTool {
    pub fn new(
        skill_registry: Arc<SkillRegistry>,
        session_store: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            skill_registry,
            session_store,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "close_project".to_string(),
            description:
                "Close the active project. Clears the working directory, unloads project-specific skills, and unbinds the session."
                    .to_string(),
            parameters: schema_for::<CloseProjectInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct CloseProjectInput {}

#[derive(Serialize)]
struct CloseProjectOutput {
    project_closed: String,
    skills_unloaded: usize,
    message: String,
}

#[async_trait::async_trait]
impl Tool for CloseProjectTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "close_project",
            "Close the active project",
            CloseProjectTool::spec().parameters,
        )
    }

    async fn run(&self, _arguments_json: &str) -> Result<String, ToolError> {
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

        let project_name = session
            .project_id
            .take()
            .ok_or_else(|| ToolError::Execution("no project is currently open".to_string()))?;

        // Clear session binding
        session.root_dir = None;
        session.updated_at = chrono::Utc::now();

        self.session_store
            .update(&session)
            .await
            .map_err(|e| ToolError::Execution(format!("update session: {e}")))?;

        // Unload project-scoped skills
        let skills_unloaded = self
            .skill_registry
            .unregister_by_source(&SkillSource::Project(project_name.clone()));

        let output = CloseProjectOutput {
            project_closed: project_name.clone(),
            skills_unloaded,
            message: format!(
                "Project '{project_name}' closed. {skills_unloaded} project skill(s) unloaded."
            ),
        };

        serde_json::to_string_pretty(&output)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}
