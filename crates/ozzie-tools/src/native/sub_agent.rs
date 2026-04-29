use std::sync::Arc;

use ozzie_core::config::SubAgentConfig;
use ozzie_core::domain::{SubAgentRunner, Tool, ToolError, ToolInfo, TOOL_CTX};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Maximum sub-agent nesting depth (sub-agents cannot call other sub-agents).
const MAX_SUB_AGENT_DEPTH: u32 = 1;

/// A user-configured sub-agent exposed as a callable tool.
///
/// Each configured sub-agent becomes a tool named `agent_{name}` that the
/// parent agent can invoke to delegate specialized tasks.
pub struct SubAgentTool {
    agent_name: String,
    tool_name: String,
    config: SubAgentConfig,
    runner: Arc<dyn SubAgentRunner>,
}

impl SubAgentTool {
    pub fn new(agent_name: String, config: SubAgentConfig, runner: Arc<dyn SubAgentRunner>) -> Self {
        let tool_name = format!("agent_{}", agent_name.replace('-', "_").to_ascii_lowercase());
        Self {
            agent_name,
            tool_name,
            config,
            runner,
        }
    }

    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.tool_name.clone(),
            description: self.config.description.clone(),
            parameters: schema_for::<SubAgentInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for a sub-agent tool call.
#[derive(Deserialize, JsonSchema)]
struct SubAgentInput {
    /// Clear description of the task to delegate to this agent.
    task: String,
    /// Optional additional context to include (e.g. relevant code snippets, file paths).
    #[serde(default)]
    context: Option<String>,
}

#[async_trait::async_trait]
impl Tool for SubAgentTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            &self.tool_name,
            &self.config.description,
            self.spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: SubAgentInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid arguments: {e}")))?;

        if input.task.is_empty() {
            return Err(ToolError::Execution("task is required".into()));
        }

        // Enforce single-level nesting: sub-agents cannot call other sub-agents
        let current_depth = TOOL_CTX
            .try_with(|ctx| ctx.subtask_depth)
            .unwrap_or(0);

        if current_depth >= MAX_SUB_AGENT_DEPTH {
            return Err(ToolError::Execution(
                "sub-agents cannot call other sub-agents".into(),
            ));
        }

        let conversation_id = TOOL_CTX
            .try_with(|ctx| ctx.conversation_id.clone())
            .unwrap_or_default();

        if conversation_id.is_empty() {
            return Err(ToolError::Execution(
                "sub-agent requires a session context".into(),
            ));
        }

        let work_dir = TOOL_CTX
            .try_with(|ctx| ctx.work_dir.clone())
            .ok()
            .flatten();

        self.runner
            .run_sub_agent(
                &self.agent_name,
                &self.config,
                &input.task,
                input.context.as_deref(),
                &conversation_id,
                work_dir.as_deref(),
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_normalizes() {
        let config = SubAgentConfig {
            model: None,
            persona: "test".into(),
            description: "test agent".into(),
            tools: vec![],
            context_mode: ozzie_core::config::ContextMode::TaskOnly,
            budget: None,
        };

        struct NoopRunner;
        #[async_trait::async_trait]
        impl SubAgentRunner for NoopRunner {
            async fn run_sub_agent(
                &self, _: &str, _: &SubAgentConfig, _: &str, _: Option<&str>,
                _: &str, _: Option<&str>,
            ) -> Result<String, ToolError> {
                Ok(String::new())
            }
        }

        let tool = SubAgentTool::new("code-reviewer".into(), config, Arc::new(NoopRunner));
        assert_eq!(tool.tool_name, "agent_code_reviewer");
    }

    #[test]
    fn schema_has_required_task() {
        let schema = serde_json::to_value(schema_for::<SubAgentInput>()).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("task")));
    }
}
