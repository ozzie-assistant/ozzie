use std::sync::Arc;

use ozzie_core::domain::{SubtaskRunner, Tool, ToolError, ToolInfo, TOOL_CTX};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

const MAX_SUBTASK_DEPTH: u32 = 3;

/// Default tools available to subtasks.
pub const DEFAULT_SUBTASK_TOOLS: &[&str] = &[
    "execute",
    "git",
    "file_read",
    "file_write",
    "list_dir",
    "glob",
    "grep",
];

/// Delegates a sub-problem to an inline ReAct loop and returns the result.
pub struct RunSubtaskTool {
    runner: Arc<dyn SubtaskRunner>,
}

impl RunSubtaskTool {
    pub fn new(runner: Arc<dyn SubtaskRunner>) -> Self {
        Self { runner }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "run_subtask".to_string(),
            description: "Delegate a sub-problem to a separate agent that runs inline and returns the result. Use this when you need to break a complex task into independent sub-problems.".to_string(),
            parameters: schema_for::<RunSubtaskInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for run_subtask.
#[derive(Deserialize, JsonSchema)]
struct RunSubtaskInput {
    /// Clear instruction of what the subtask should accomplish.
    instruction: String,
    /// Tool names the subtask agent can use (defaults to file/code tools).
    #[serde(default)]
    tools: Vec<String>,
    /// Working directory for file operations.
    #[serde(default)]
    work_dir: Option<String>,
    /// LLM provider name to use (defaults to the configured default provider).
    /// Can also be resolved automatically from `tags` if not set.
    #[serde(default)]
    provider: Option<String>,
    /// Required provider tags (e.g. ["vision", "fast"]). When set and `provider`
    /// is not specified, the system picks an idle provider matching all tags.
    #[serde(default)]
    tags: Vec<String>,
}

#[async_trait::async_trait]
impl Tool for RunSubtaskTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "run_subtask",
            "Delegate a sub-problem to a separate inline agent",
            RunSubtaskTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: RunSubtaskInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        if input.instruction.is_empty() {
            return Err(ToolError::Execution("instruction is required".to_string()));
        }

        // Check recursion depth from caller context
        let current_depth = TOOL_CTX
            .try_with(|ctx| ctx.subtask_depth)
            .unwrap_or(0);

        if current_depth >= MAX_SUBTASK_DEPTH {
            return Err(ToolError::Execution(format!(
                "maximum subtask depth ({MAX_SUBTASK_DEPTH}) reached"
            )));
        }

        let tools: Vec<String> = if input.tools.is_empty() {
            DEFAULT_SUBTASK_TOOLS.iter().map(|s| s.to_string()).collect()
        } else {
            input.tools
        };

        // Resolve work_dir: explicit > inherited from caller > none
        let work_dir = input
            .work_dir
            .or_else(|| TOOL_CTX.try_with(|ctx| ctx.work_dir.clone()).ok().flatten());

        let result = self
            .runner
            .run_subtask(
                &input.instruction,
                &tools,
                work_dir.as_deref(),
                current_depth + 1,
                input.provider.as_deref(),
                &input.tags,
            )
            .await?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_required_instruction() {
        let schema = serde_json::to_value(schema_for::<RunSubtaskInput>()).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("instruction")));
    }
}
