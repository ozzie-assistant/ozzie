use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::ToolSpec;

/// Input for the yield_control tool.
///
/// The LLM calls this tool to voluntarily stop the ReAct loop.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum YieldInput {
    /// Task is complete — stop the loop.
    Done,
    /// Waiting for an external event before continuing.
    Waiting {
        /// Event type to resume on (e.g. "task.completed").
        #[serde(default)]
        resume_on: Option<String>,
    },
    /// Save progress and yield to pending work.
    Checkpoint {
        /// Summary of progress so far.
        #[serde(default)]
        summary: Option<String>,
    },
}

pub struct YieldControlTool;

impl YieldControlTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "yield_control".to_string(),
            description: "Voluntarily stop the current ReAct loop. Use 'done' when the task is complete, 'waiting' when blocked on external input, 'checkpoint' to save progress and yield.".to_string(),
            parameters: schemars::schema_for!(YieldInput),
            dangerous: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for YieldControlTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "yield_control",
            "Voluntarily stop the current ReAct loop.",
            Self::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        // Validate input — the actual loop exit is handled by the ReactLoop,
        // which detects the tool name and short-circuits.
        let _input: YieldInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("invalid yield_control input: {e}")))?;
        Ok("yield acknowledged".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_valid() {
        let spec = YieldControlTool::spec();
        assert_eq!(spec.name, "yield_control");
        assert!(!spec.dangerous);
    }

    #[tokio::test]
    async fn yield_done() {
        let tool = YieldControlTool;
        let result = tool.run(r#"{"reason": "done"}"#).await.unwrap();
        assert_eq!(result, "yield acknowledged");
    }

    #[tokio::test]
    async fn yield_waiting_with_resume() {
        let tool = YieldControlTool;
        let result = tool
            .run(r#"{"reason": "waiting", "resume_on": "task.completed"}"#)
            .await
            .unwrap();
        assert_eq!(result, "yield acknowledged");
    }

    #[tokio::test]
    async fn yield_checkpoint() {
        let tool = YieldControlTool;
        let result = tool
            .run(r#"{"reason": "checkpoint", "summary": "step 3 done"}"#)
            .await
            .unwrap();
        assert_eq!(result, "yield acknowledged");
    }

    #[tokio::test]
    async fn yield_invalid_input() {
        let tool = YieldControlTool;
        let result = tool.run(r#"{"reason": "invalid_reason"}"#).await;
        assert!(result.is_err());
    }
}
