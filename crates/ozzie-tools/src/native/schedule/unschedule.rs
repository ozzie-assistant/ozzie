use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_core::events::{EventBus, EventPayload, EventSource};
use ozzie_core::domain::SchedulerPort;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Removes a dynamic schedule entry.
pub struct UnscheduleTaskTool {
    scheduler: Arc<dyn SchedulerPort>,
    bus: Arc<dyn EventBus>,
}

impl UnscheduleTaskTool {
    pub fn new(scheduler: Arc<dyn SchedulerPort>, bus: Arc<dyn EventBus>) -> Self {
        Self { scheduler, bus }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "unschedule_task".to_string(),
            description: "Remove a dynamic schedule entry by its ID. Skill-based schedules cannot be removed.".to_string(),
            parameters: schema_for::<UnscheduleInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for unschedule_task.
#[derive(Deserialize, JsonSchema)]
struct UnscheduleInput {
    /// The schedule entry ID to remove (sched_... prefix).
    entry_id: String,
}

#[async_trait::async_trait]
impl Tool for UnscheduleTaskTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "unschedule_task",
            "Remove a dynamic schedule entry",
            UnscheduleTaskTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: UnscheduleInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("unschedule_task: parse input: {e}")))?;

        if input.entry_id.is_empty() {
            return Err(ToolError::Execution(
                "unschedule_task: entry_id is required".to_string(),
            ));
        }

        let entry = self
            .scheduler
            .get_entry(&input.entry_id)
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "unschedule_task: entry not found: {}",
                    input.entry_id
                ))
            })?;

        if entry.source.is_skill() {
            return Err(ToolError::Execution(format!(
                "unschedule_task: cannot remove skill-based schedule {:?} (managed by skill registry)",
                input.entry_id
            )));
        }

        let title = entry.title.clone();

        if !self.scheduler.remove_entry(&input.entry_id) {
            return Err(ToolError::Execution(format!(
                "unschedule_task: failed to remove entry: {}",
                input.entry_id
            )));
        }

        self.bus.publish(ozzie_core::events::Event::new(
            EventSource::Scheduler,
            EventPayload::ScheduleRemoved {
                entry_id: input.entry_id.clone(),
                title,
            },
        ));

        let result = serde_json::json!({
            "entry_id": input.entry_id,
            "status": "removed",
        });
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::{make_bus, make_scheduler};
    use chrono::Utc;
    use ozzie_core::domain::{ScheduleEntry, ScheduleSource, TaskTemplate};

    fn make_entry(id: &str, source: &str) -> ScheduleEntry {
        let source = match source {
            "skill" => ScheduleSource::Skill {
                skill_name: id.to_string(),
            },
            _ => ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: id.to_string(),
                    description: "test".to_string(),
                    tools: Vec::new(),
                    work_dir: None,
                    env: std::collections::HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
        };
        ScheduleEntry {
            id: id.to_string(),
            source,
            conversation_id: None,
            title: id.to_string(),
            description: "test".to_string(),
            cron_spec: Some("0 * * * *".to_string()),
            interval_sec: 0,
            on_event: None,
            cooldown_sec: 60,
            max_runs: 0,
            run_count: 0,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        }
    }

    #[tokio::test]
    async fn unschedule_dynamic_entry() {
        let sched = make_scheduler();
        let bus = make_bus();
        sched.add_entry(make_entry("sched_123", "dynamic")).unwrap();

        let tool = UnscheduleTaskTool::new(sched.clone(), bus);
        let result = tool.run(r#"{"entry_id":"sched_123"}"#).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "removed");
        assert_eq!(sched.list_entries().len(), 0);
    }

    #[tokio::test]
    async fn unschedule_skill_entry_rejected() {
        let sched = make_scheduler();
        let bus = make_bus();
        sched
            .add_entry(make_entry("skill_deploy", "skill"))
            .unwrap();

        let tool = UnscheduleTaskTool::new(sched, bus);
        let result = tool.run(r#"{"entry_id":"skill_deploy"}"#).await;
        assert!(result.unwrap_err().to_string().contains("skill-based schedule"));
    }

    #[tokio::test]
    async fn unschedule_not_found() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = UnscheduleTaskTool::new(sched, bus);

        let result = tool.run(r#"{"entry_id":"sched_nonexistent"}"#).await;
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn unschedule_missing_id() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = UnscheduleTaskTool::new(sched, bus);

        let result = tool.run(r#"{"entry_id":""}"#).await;
        assert!(result.unwrap_err().to_string().contains("required"));
    }
}
