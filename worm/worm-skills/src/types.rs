use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Tracks where a skill was loaded from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    /// Loaded from `$OZZIE_PATH/skills/` or config additional dirs.
    #[default]
    Global,
    /// Loaded from a project's `.ozzie/skills/` directory.
    Project(String),
    /// Created at runtime by the agent.
    Agent,
}

/// Loaded skill definition (from SKILL.md + optional workflow.yaml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMD {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Markdown body from SKILL.md.
    #[serde(default)]
    pub body: String,
    /// Directory containing the skill files.
    #[serde(default)]
    pub dir: String,
    /// Optional structured workflow.
    #[serde(default)]
    pub workflow: Option<WorkflowDef>,
    /// Optional schedule triggers.
    #[serde(default)]
    pub triggers: Option<TriggersDef>,
    /// Where this skill was loaded from.
    #[serde(skip, default)]
    pub source: SkillSource,
}

/// YAML workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub vars: HashMap<String, VarDef>,
    pub steps: Vec<StepDef>,
}

/// Variable definition for workflow inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarDef {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

/// A single step in a workflow DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub instruction: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub acceptance: Option<AcceptanceDef>,
}

/// Acceptance criteria for step verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceDef {
    pub criteria: Vec<String>,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: usize,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_max_attempts() -> usize {
    2
}

/// Schedule triggers definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggersDef {
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub interval_sec: Option<u64>,
    #[serde(default)]
    pub on_event: Option<EventTriggerDef>,
    #[serde(default = "default_cooldown")]
    pub cooldown_sec: u64,
    #[serde(default)]
    pub max_runs: Option<u64>,
}

fn default_cooldown() -> u64 {
    60
}

/// Event trigger definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTriggerDef {
    pub event: String,
    #[serde(default)]
    pub filter: HashMap<String, String>,
}
