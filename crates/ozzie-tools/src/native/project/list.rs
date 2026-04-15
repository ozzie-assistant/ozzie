use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_core::project::ProjectRegistry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Lists all discovered projects.
pub struct ListProjectsTool {
    registry: Arc<ProjectRegistry>,
}

impl ListProjectsTool {
    pub fn new(registry: Arc<ProjectRegistry>) -> Self {
        Self { registry }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "list_projects".to_string(),
            description: "List all available projects with their descriptions and paths."
                .to_string(),
            parameters: schema_for::<ListProjectsInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ListProjectsInput {
    /// Optional tag filter — only return projects matching this tag.
    #[serde(default)]
    tag: Option<String>,
}

#[derive(Serialize)]
struct ProjectEntry {
    name: String,
    description: String,
    path: String,
    tags: Vec<String>,
    skills: Vec<String>,
}

#[async_trait::async_trait]
impl Tool for ListProjectsTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "list_projects",
            "List available projects",
            ListProjectsTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: ListProjectsInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        let projects = self.registry.all();

        let entries: Vec<ProjectEntry> = projects
            .into_iter()
            .filter(|p| {
                input
                    .tag
                    .as_ref()
                    .is_none_or(|tag| p.tags.contains(tag))
            })
            .map(|p| ProjectEntry {
                name: p.name,
                description: p.description,
                path: p.path,
                tags: p.tags,
                skills: p.skills,
            })
            .collect();

        serde_json::to_string_pretty(&entries)
            .map_err(|e| ToolError::Execution(format!("serialize: {e}")))
    }
}
