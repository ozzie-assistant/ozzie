use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tracing::info;

use crate::domain::{
    Message, RunnerFactory, RunnerOpts, SkillError, SkillExecutor, Tool, ToolLookup,
};
use crate::events::EventBus;

use super::dag::DAG;
use super::types::{SkillMD, StepDef, VarDef};

/// Configuration for running skills.
pub struct RunnerConfig {
    pub runner_factory: Arc<dyn RunnerFactory>,
    pub tool_lookup: Arc<dyn ToolLookup>,
    pub bus: Arc<dyn EventBus>,
}

/// Executes a workflow skill by running its DAG of steps.
pub struct WorkflowRunner {
    skill_name: String,
    model: Option<String>,
    vars: HashMap<String, VarDef>,
    dag: DAG,
    cfg: RunnerConfig,
}

impl WorkflowRunner {
    /// Creates a runner from a SkillMD with a workflow definition.
    pub fn from_skill(skill: &SkillMD, cfg: RunnerConfig) -> Result<Self, SkillError> {
        let workflow = skill
            .workflow
            .as_ref()
            .ok_or_else(|| SkillError::NotFound(format!("skill '{}' has no workflow", skill.name)))?;

        let dag = DAG::new(workflow.steps.clone())
            .map_err(|e| SkillError::Execution(format!("build DAG for '{}': {e}", skill.name)))?;

        Ok(Self {
            skill_name: skill.name.clone(),
            model: workflow.model.clone(),
            vars: workflow.vars.clone(),
            dag,
            cfg,
        })
    }

    /// Runs the workflow, executing steps in DAG order with parallel execution
    /// where dependencies allow. Returns the output of the last step.
    pub async fn run(&self, vars: HashMap<String, String>) -> Result<String, SkillError> {
        let mut vars = vars;
        self.validate_vars(&vars)?;

        // Apply defaults
        for (name, v) in &self.vars {
            if !vars.contains_key(name)
                && let Some(ref default) = v.default {
                    vars.insert(name.clone(), default.clone());
                }
        }

        let mut completed = HashSet::new();
        let mut results: HashMap<String, String> = HashMap::new();

        loop {
            let ready = self.dag.ready_steps(&completed);
            if ready.is_empty() {
                break;
            }

            // Run ready steps in parallel
            let mut handles = Vec::new();
            for step_id in &ready {
                let step = self.dag.get_step(step_id).cloned().ok_or_else(|| {
                    SkillError::Execution(format!("step '{step_id}' not found in DAG"))
                })?;

                let results_copy = results.clone();
                let vars_copy = vars.clone();
                let skill_name = self.skill_name.clone();
                let model = step.model.clone().or_else(|| self.model.clone());
                let runner_factory = self.cfg.runner_factory.clone();
                let tool_lookup = self.cfg.tool_lookup.clone();

                handles.push(tokio::spawn(async move {
                    run_step(
                        &skill_name,
                        &step,
                        model.as_deref(),
                        &vars_copy,
                        &results_copy,
                        runner_factory.as_ref(),
                        tool_lookup.as_ref(),
                    )
                    .await
                    .map(|output| (step.id.clone(), output))
                }));
            }

            // Collect results, fail-fast on first error
            for handle in handles {
                let (step_id, output) = handle
                    .await
                    .map_err(|e| SkillError::Execution(format!("join: {e}")))?
                    .map_err(|e| SkillError::Execution(e.to_string()))?;
                completed.insert(step_id.clone());
                results.insert(step_id, output);
            }
        }

        // Return the output of the last step in topological order
        let order = self.dag.topological_order();
        if order.is_empty() {
            return Ok(String::new());
        }
        Ok(results
            .get(order.last().unwrap())
            .cloned()
            .unwrap_or_default())
    }

    fn validate_vars(&self, vars: &HashMap<String, String>) -> Result<(), SkillError> {
        for (name, v) in &self.vars {
            if v.required && !vars.contains_key(name) {
                return Err(SkillError::Execution(format!(
                    "skill '{}': required variable '{}' not provided",
                    self.skill_name, name
                )));
            }
        }
        Ok(())
    }
}

async fn run_step(
    skill_name: &str,
    step: &StepDef,
    model: Option<&str>,
    vars: &HashMap<String, String>,
    prev_results: &HashMap<String, String>,
    runner_factory: &dyn RunnerFactory,
    tool_lookup: &dyn ToolLookup,
) -> Result<String, SkillError> {
    let model_name = model.unwrap_or("default");

    info!(skill = skill_name, step = %step.id, "running step");

    // Resolve tools
    let tools: Vec<Box<dyn Tool>> = if step.tools.is_empty() {
        Vec::new()
    } else {
        tool_lookup.tools_by_names(&step.tools)
    };

    // Build instruction
    let instruction = build_step_instruction(step, vars, prev_results);

    let runner = runner_factory
        .create_runner(model_name, &instruction, tools, RunnerOpts::default())
        .await
        .map_err(|e| SkillError::Execution(format!("create runner for step '{}': {e}", step.id)))?;

    let messages = vec![Message::user("Execute this step.")];

    let output = runner
        .run(messages)
        .await
        .map_err(|e| SkillError::Execution(format!("step '{}': {e}", step.id)))?;

    info!(skill = skill_name, step = %step.id, "step completed");

    Ok(output)
}

fn build_step_instruction(
    step: &StepDef,
    vars: &HashMap<String, String>,
    prev_results: &HashMap<String, String>,
) -> String {
    let mut buf = step.instruction.clone();

    // Inject variables
    if !vars.is_empty() {
        buf.push_str("\n\n## Variables\n\n");
        for (name, value) in vars {
            buf.push_str(&format!("- **{name}**: {value}\n"));
        }
    }

    // Inject previous step results
    if !step.needs.is_empty() {
        buf.push_str("\n\n## Previous Step Results\n\n");
        for need in &step.needs {
            if let Some(result) = prev_results.get(need) {
                buf.push_str(&format!("### Step: {need}\n\n{result}\n\n"));
            }
        }
    }

    // Acceptance criteria
    if let Some(ref acceptance) = step.acceptance {
        buf.push_str("\n\n## Acceptance Criteria\n\n");
        for c in &acceptance.criteria {
            buf.push_str(&format!("- {c}\n"));
        }
    }

    buf
}

/// Adapter that implements SkillExecutor using the WorkflowRunner.
pub struct SkillExecutorAdapter {
    registry: Arc<super::SkillRegistry>,
    cfg: RunnerConfig,
}

impl SkillExecutorAdapter {
    pub fn new(registry: Arc<super::SkillRegistry>, cfg: RunnerConfig) -> Self {
        Self { registry, cfg }
    }
}

#[async_trait::async_trait]
impl SkillExecutor for SkillExecutorAdapter {
    async fn run_skill(
        &self,
        skill_name: &str,
        vars: HashMap<String, String>,
    ) -> Result<String, SkillError> {
        let skill = self
            .registry
            .get(skill_name)
            .ok_or_else(|| SkillError::NotFound(skill_name.to_string()))?;

        // For now, WorkflowRunner requires ownership of RunnerConfig.
        // We share the arcs so this is cheap.
        let cfg = RunnerConfig {
            runner_factory: self.cfg.runner_factory.clone(),
            tool_lookup: self.cfg.tool_lookup.clone(),
            bus: self.cfg.bus.clone(),
        };

        let runner = WorkflowRunner::from_skill(&skill, cfg)?;
        runner.run(vars).await
    }
}
