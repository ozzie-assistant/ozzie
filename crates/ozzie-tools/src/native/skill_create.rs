use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo, TOOL_CTX};
use ozzie_core::domain::SessionStore;
use ozzie_core::project::ProjectRegistry;
use ozzie_core::skills::{FsSkillRepository, SkillRegistry, SkillRepository, SkillSource};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Creates a new skill on disk and registers it in the SkillRegistry.
///
/// If a project is active, the skill is created in the project's `.ozzie/skills/` directory
/// and scoped to that project. Otherwise, it's created in `$OZZIE_PATH/skills/` as a global skill.
pub struct CreateSkillTool {
    skill_registry: Arc<SkillRegistry>,
    project_registry: Arc<ProjectRegistry>,
    session_store: Arc<dyn SessionStore>,
    skills_path: std::path::PathBuf,
}

impl CreateSkillTool {
    pub fn new(
        skill_registry: Arc<SkillRegistry>,
        project_registry: Arc<ProjectRegistry>,
        session_store: Arc<dyn SessionStore>,
        skills_path: std::path::PathBuf,
    ) -> Self {
        Self {
            skill_registry,
            project_registry,
            session_store,
            skills_path,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "create_skill".to_string(),
            description:
                "Create a new skill (SKILL.md). If a project is active, the skill is scoped to that project. Otherwise, it's created as a global skill. The agent can use this to learn new capabilities."
                    .to_string(),
            parameters: schema_for::<CreateSkillInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct CreateSkillInput {
    /// Skill name (used as directory name, e.g. "log-session").
    name: String,
    /// Short description of what this skill does.
    description: String,
    /// Markdown body — instructions for the agent when this skill is activated.
    body: String,
    /// Tool names this skill is allowed to use.
    #[serde(default)]
    allowed_tools: Vec<String>,
    /// Force global scope even if a project is active.
    #[serde(default)]
    global: bool,
}

#[derive(Serialize)]
struct CreateSkillOutput {
    name: String,
    scope: String,
    path: String,
    message: String,
}

#[async_trait::async_trait]
impl Tool for CreateSkillTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "create_skill",
            "Create a new skill",
            CreateSkillTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: CreateSkillInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        // Validate name
        if input.name.is_empty()
            || input.name.contains('/')
            || input.name.contains('\\')
            || input.name.starts_with('.')
        {
            return Err(ToolError::Execution(
                "invalid skill name: must not be empty, start with '.', or contain path separators"
                    .to_string(),
            ));
        }

        // Check duplicates
        if self.skill_registry.get(&input.name).is_some() {
            return Err(ToolError::Execution(format!(
                "skill '{}' already exists",
                input.name
            )));
        }

        // Determine scope: project-local or global
        let (skill_dir, source, scope_label) = if !input.global {
            if let Some((project_name, project_path)) = self.resolve_active_project().await? {
                let dir = std::path::PathBuf::from(&project_path)
                    .join(".ozzie/skills")
                    .join(&input.name);
                let source = SkillSource::Project(project_name);
                (dir, source, "project")
            } else {
                (
                    self.skills_path.join(&input.name),
                    SkillSource::Agent,
                    "global",
                )
            }
        } else {
            (
                self.skills_path.join(&input.name),
                SkillSource::Agent,
                "global",
            )
        };

        // Create directory
        std::fs::create_dir_all(&skill_dir)
            .map_err(|e| ToolError::Execution(format!("create skill directory: {e}")))?;

        // Write SKILL.md
        let skill_md_content = build_skill_md(&input);
        let skill_md_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_md_path, &skill_md_content)
            .map_err(|e| ToolError::Execution(format!("write SKILL.md: {e}")))?;

        // Parse and register
        let skill_repo = FsSkillRepository::new(&skill_dir);
        match skill_repo.load_one(&skill_md_path).await {
            Ok(mut skill) => {
                skill.source = source;
                self.skill_registry.register(skill);
            }
            Err(e) => {
                return Err(ToolError::Execution(format!(
                    "skill created on disk but failed to parse: {e}"
                )));
            }
        }

        let output = CreateSkillOutput {
            name: input.name,
            scope: scope_label.to_string(),
            path: skill_dir.to_string_lossy().to_string(),
            message: format!("Skill created ({scope_label})."),
        };

        serde_json::to_string_pretty(&output)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}

impl CreateSkillTool {
    /// Resolves the active project for the current session, if any.
    async fn resolve_active_project(&self) -> Result<Option<(String, String)>, ToolError> {
        let session_id = TOOL_CTX
            .try_with(|ctx| ctx.session_id.clone())
            .unwrap_or_default();

        if session_id.is_empty() {
            return Ok(None);
        }

        let session = self
            .session_store
            .get(&session_id)
            .await
            .map_err(|e| ToolError::Execution(format!("get session: {e}")))?;

        let Some(session) = session else {
            return Ok(None);
        };

        let Some(ref project_id) = session.project_id else {
            return Ok(None);
        };

        let project = self.project_registry.get(project_id);
        Ok(project.map(|p| (p.name, p.path)))
    }
}

fn build_skill_md(input: &CreateSkillInput) -> String {
    let mut content = format!("---\nname: {}\ndescription: {}\n", input.name, input.description);

    if !input.allowed_tools.is_empty() {
        content.push_str("allowed_tools:\n");
        for tool in &input.allowed_tools {
            content.push_str(&format!("  - {tool}\n"));
        }
    }

    content.push_str("---\n\n");
    content.push_str(&input.body);
    content.push('\n');
    content
}
