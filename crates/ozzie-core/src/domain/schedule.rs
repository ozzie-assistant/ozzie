use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::Event;

/// Event-based trigger definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTrigger {
    /// Event type string to match (e.g. "task.completed").
    pub event_type: String,
    /// Optional key-value filters on the event payload.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub filter: HashMap<String, String>,
}

impl EventTrigger {
    /// Returns true if the given event matches this trigger.
    pub fn matches(&self, event: &Event) -> bool {
        if event.event_type() != self.event_type {
            return false;
        }

        // Match filters against serialized event payload.
        // Uses serde_json round-trip because EventPayload is a large tagged enum —
        // extracting arbitrary top-level fields without serialization would require
        // a dedicated accessor for every variant, which isn't worth the complexity.
        if !self.filter.is_empty() {
            let Ok(payload_value) = serde_json::to_value(&event.payload) else {
                return false;
            };
            for (key, expected) in &self.filter {
                match payload_value.get(key) {
                    Some(serde_json::Value::String(val)) if val == expected => {}
                    _ => return false,
                }
            }
        }

        true
    }
}

/// Task template for scheduled execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTemplate {
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_dir: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tools: Vec<String>,
}

/// Discriminator for where a schedule entry originated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum ScheduleSource {
    Skill { skill_name: String },
    Dynamic { task_template: TaskTemplate },
}

impl ScheduleSource {
    /// Returns the source tag as a string (for event payloads / display).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Skill { .. } => "skill",
            Self::Dynamic { .. } => "dynamic",
        }
    }

    /// Returns the task template, if this is a dynamic schedule.
    pub fn task_template(&self) -> Option<&TaskTemplate> {
        match self {
            Self::Dynamic { task_template } => Some(task_template),
            Self::Skill { .. } => None,
        }
    }

    /// Returns the skill name, if this is a skill-based schedule.
    pub fn skill_name(&self) -> Option<&str> {
        match self {
            Self::Skill { skill_name } => Some(skill_name),
            Self::Dynamic { .. } => None,
        }
    }

    /// Returns true if this is a skill-based schedule.
    pub fn is_skill(&self) -> bool {
        matches!(self, Self::Skill { .. })
    }
}

/// A persistent schedule entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub id: String,
    #[serde(flatten)]
    pub source: ScheduleSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub title: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_spec: Option<String>,
    #[serde(default)]
    pub interval_sec: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_event: Option<EventTrigger>,
    #[serde(default)]
    pub cooldown_sec: u64,
    #[serde(default)]
    pub max_runs: u32,
    #[serde(default)]
    pub run_count: u32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Callback trait for schedule triggers.
#[async_trait::async_trait]
pub trait ScheduleHandler: Send + Sync {
    async fn on_trigger(&self, entry: &ScheduleEntry, trigger: &str);
}

/// Domain port for schedule management.
pub trait SchedulerPort: Send + Sync {
    fn add_entry(&self, entry: ScheduleEntry) -> Result<(), SchedulerError>;
    fn remove_entry(&self, id: &str) -> bool;
    fn list_entries(&self) -> Vec<ScheduleEntry>;
    fn get_entry(&self, id: &str) -> Option<ScheduleEntry>;
    fn trigger_entry(&self, id: &str) -> Result<ScheduleEntry, SchedulerError>;
    fn set_enabled(&self, id: &str, enabled: bool) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("invalid cron: {0}")]
    InvalidCron(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}
