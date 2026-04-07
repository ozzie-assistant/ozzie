#[cfg(test)]
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo, ToolSet, TOOL_CTX};
use ozzie_core::skills::SkillRegistry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolRegistry, ToolSpec};

/// Allows the agent to activate tools or skills at runtime.
///
/// For tools: marks them active for the session so they appear on the next turn.
/// For skills: loads the skill body (instructions), auto-activates allowed tools,
/// and activates `run_workflow` if the skill has a workflow.
pub struct ActivateTool {
    tool_set: Arc<ToolSet>,
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Option<Arc<SkillRegistry>>,
}

impl ActivateTool {
    pub fn new(
        tool_set: Arc<ToolSet>,
        tool_registry: Arc<ToolRegistry>,
        skill_registry: Option<Arc<SkillRegistry>>,
    ) -> Self {
        Self {
            tool_set,
            tool_registry,
            skill_registry,
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "activate".to_string(),
            description: "Activate additional tools or skills to make them available. For tools: activates them for use on the next message. For skills: loads the skill's instructions and activates its allowed tools. Skills with a workflow will also activate run_workflow.".to_string(),
            parameters: schema_for::<ActivateInput>(),
            dangerous: false,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ActivateInput {
    /// List of tool or skill names to activate (e.g. ["docker_build", "deploy"]).
    names: Vec<String>,
}

#[derive(Serialize)]
struct ActivateOutput {
    activated: Vec<ActivatedEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
}

#[derive(Serialize)]
struct ActivatedEntry {
    name: String,
    #[serde(rename = "type")]
    entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resources: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    has_workflow: bool,
}

#[async_trait::async_trait]
impl Tool for ActivateTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "activate",
            "Activate tools or skills for the current session",
            ActivateTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        // Get session_id from task-local context
        let session_id = TOOL_CTX
            .try_with(|ctx| ctx.session_id.clone())
            .map_err(|_| ToolError::Execution("activate: no session in context".to_string()))?;

        if session_id.is_empty() {
            return Err(ToolError::Execution(
                "activate: no session in context".to_string(),
            ));
        }

        let input: ActivateInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("activate: parse input: {e}")))?;

        if input.names.is_empty() {
            return Err(ToolError::Execution(
                "activate: names list is empty".to_string(),
            ));
        }

        let mut output = ActivateOutput {
            activated: Vec::new(),
            errors: Vec::new(),
        };

        for name in &input.names {
            // Try tool first
            if self.tool_set.is_known(name) {
                if self.tool_set.activate(&session_id, name) {
                    let desc = self.tool_registry.spec(name).map(|s| s.description);
                    output.activated.push(ActivatedEntry {
                        name: name.clone(),
                        entry_type: "tool".to_string(),
                        description: desc,
                        body: None,
                        tools: Vec::new(),
                        resources: Vec::new(),
                        has_workflow: false,
                    });
                } else {
                    output
                        .errors
                        .push(format!("failed to activate tool: {name:?}"));
                }
                continue;
            }

            // Try skill
            if let Some(ref skill_reg) = self.skill_registry
                && let Some(skill) = skill_reg.get(name)
            {
                let has_workflow = skill.workflow.is_some();
                let mut activated_tools = Vec::new();

                // Activate allowed tools from the skill
                for tool_name in &skill.allowed_tools {
                    if self.tool_set.is_known(tool_name)
                        && self.tool_set.activate(&session_id, tool_name)
                    {
                        activated_tools.push(tool_name.clone());
                    }
                }

                // Auto-activate run_workflow if the skill has a workflow
                if has_workflow
                    && self.tool_set.is_known("run_workflow")
                    && self.tool_set.activate(&session_id, "run_workflow")
                {
                    activated_tools.push("run_workflow".to_string());
                }

                let resources = list_resources(&skill.dir);

                output.activated.push(ActivatedEntry {
                    name: name.clone(),
                    entry_type: "skill".to_string(),
                    description: Some(skill.description.clone()),
                    body: if skill.body.is_empty() {
                        None
                    } else {
                        Some(skill.body.clone())
                    },
                    tools: activated_tools,
                    resources,
                    has_workflow,
                });
                continue;
            }

            output
                .errors
                .push(format!("unknown tool or skill: {name:?}"));
        }

        serde_json::to_string(&output)
            .map_err(|e| ToolError::Execution(format!("activate: marshal result: {e}")))
    }
}

/// Scans optional subdirectories (scripts, references, assets) for resources.
fn list_resources(dir: &str) -> Vec<String> {
    if dir.is_empty() {
        return Vec::new();
    }

    let mut resources = Vec::new();
    for sub_dir in &["scripts", "references", "assets"] {
        let full_path = Path::new(dir).join(sub_dir);
        let entries = match std::fs::read_dir(&full_path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                && let Some(name) = entry.file_name().to_str()
            {
                resources.push(format!("{sub_dir}/{name}"));
            }
        }
    }
    resources
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool_set() -> Arc<ToolSet> {
        Arc::new(ToolSet::new(
            &["execute", "file_read"],
            &[
                "execute",
                "file_read",
                "docker_build",
                "web_search",
                "run_workflow",
            ],
        ))
    }

    fn make_registry() -> Arc<ToolRegistry> {
        let reg = Arc::new(ToolRegistry::new());
        // Register some specs for description lookup
        use schemars::schema::{InstanceType, RootSchema, SchemaObject, SingleOrVec};
        let empty_schema = || RootSchema {
            schema: SchemaObject {
                instance_type: Some(SingleOrVec::Single(Box::new(InstanceType::Object))),
                ..Default::default()
            },
            ..Default::default()
        };
        reg.register(
            Box::new(DummyTool("docker_build")),
            ToolSpec {
                name: "docker_build".to_string(),
                description: "Build Docker images".to_string(),
                parameters: empty_schema(),
                dangerous: false,
            },
        );
        reg.register(
            Box::new(DummyTool("web_search")),
            ToolSpec {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                parameters: empty_schema(),
                dangerous: false,
            },
        );
        reg
    }

    fn make_skill_registry() -> Arc<SkillRegistry> {
        let reg = Arc::new(SkillRegistry::new());
        reg.register(ozzie_core::skills::SkillMD {
            name: "deploy".to_string(),
            description: "Deploy to production".to_string(),
            license: None,
            compatibility: None,
            metadata: HashMap::new(),
            allowed_tools: vec!["docker_build".to_string()],
            body: "## Deploy\nRun docker build then push.".to_string(),
            dir: String::new(),
            workflow: Some(ozzie_core::skills::WorkflowDef {
                model: None,
                vars: HashMap::new(),
                steps: vec![],
            }),
            triggers: None,
        });
        reg
    }

    struct DummyTool(&'static str);

    #[async_trait::async_trait]
    impl Tool for DummyTool {
        fn info(&self) -> ToolInfo {
            ToolInfo::new(self.0, format!("Dummy {}", self.0))
        }

        async fn run(&self, _args: &str) -> Result<String, ToolError> {
            Ok("ok".to_string())
        }
    }

    /// Helper to run a test inside TOOL_CTX scope.
    async fn with_session<F, Fut>(session_id: &str, f: F) -> Result<String, ToolError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<String, ToolError>>,
    {
        let ctx = ozzie_core::domain::ToolContext {
            session_id: session_id.to_string(),
            ..Default::default()
        };
        TOOL_CTX.scope(ctx, f()).await
    }

    #[tokio::test]
    async fn activate_known_tool() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts.clone(), reg, None);

        let result = with_session("s1", || tool.run(r#"{"names":["docker_build"]}"#))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["activated"][0]["name"], "docker_build");
        assert_eq!(parsed["activated"][0]["type"], "tool");
        assert_eq!(parsed["activated"][0]["description"], "Build Docker images");

        // Verify tool is now active
        assert!(ts.is_active("s1", "docker_build"));
        assert!(ts.activated_during_turn("s1"));
    }

    #[tokio::test]
    async fn activate_unknown_name() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts, reg, None);

        let result = with_session("s1", || tool.run(r#"{"names":["nonexistent"]}"#))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["activated"].as_array().unwrap().is_empty());
        assert!(!parsed["errors"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn activate_skill_with_workflow() {
        let ts = make_tool_set();
        let reg = make_registry();
        let skill_reg = make_skill_registry();
        let tool = ActivateTool::new(ts.clone(), reg, Some(skill_reg));

        let result = with_session("s1", || tool.run(r#"{"names":["deploy"]}"#))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let entry = &parsed["activated"][0];
        assert_eq!(entry["name"], "deploy");
        assert_eq!(entry["type"], "skill");
        assert_eq!(entry["has_workflow"], true);
        assert!(entry["body"].as_str().unwrap().contains("Deploy"));

        // Allowed tools should be activated
        assert!(ts.is_active("s1", "docker_build"));
        // run_workflow should be auto-activated
        assert!(ts.is_active("s1", "run_workflow"));

        let tools = entry["tools"].as_array().unwrap();
        let tool_names: Vec<&str> = tools.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(tool_names.contains(&"docker_build"));
        assert!(tool_names.contains(&"run_workflow"));
    }

    #[tokio::test]
    async fn activate_multiple_names() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts.clone(), reg, None);

        let result = with_session("s1", || {
            tool.run(r#"{"names":["docker_build","web_search","nope"]}"#)
        })
        .await
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["activated"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn activate_empty_names() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts, reg, None);

        let result = with_session("s1", || tool.run(r#"{"names":[]}"#)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn activate_no_session_context() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts, reg, None);

        // Call without TOOL_CTX scope
        let result = tool.run(r#"{"names":["docker_build"]}"#).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no session"));
    }

    #[tokio::test]
    async fn activate_invalid_json() {
        let ts = make_tool_set();
        let reg = make_registry();
        let tool = ActivateTool::new(ts, reg, None);

        let result = with_session("s1", || tool.run("not json")).await;
        assert!(result.is_err());
    }
}
