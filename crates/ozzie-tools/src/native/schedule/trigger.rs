use std::sync::Arc;

use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_core::domain::SchedulerPort;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Manually triggers an existing schedule entry.
pub struct TriggerScheduleTool {
    scheduler: Arc<dyn SchedulerPort>,
}

impl TriggerScheduleTool {
    pub fn new(scheduler: Arc<dyn SchedulerPort>) -> Self {
        Self { scheduler }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "trigger_schedule".to_string(),
            description: "Manually trigger an existing schedule entry, bypassing its cron/interval/event trigger and cooldown. Use list_schedules to find available entry IDs.".to_string(),
            parameters: schema_for::<TriggerScheduleInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for trigger_schedule.
#[derive(Deserialize, JsonSchema)]
struct TriggerScheduleInput {
    /// The schedule entry ID to trigger (sched_... or skill_... prefix).
    entry_id: String,
}

#[async_trait::async_trait]
impl Tool for TriggerScheduleTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "trigger_schedule",
            "Manually trigger an existing schedule entry",
            TriggerScheduleTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: TriggerScheduleInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("trigger_schedule: parse input: {e}")))?;

        if input.entry_id.is_empty() {
            return Err(ToolError::Execution(
                "trigger_schedule: entry_id is required".to_string(),
            ));
        }

        let entry = self
            .scheduler
            .trigger_entry(&input.entry_id)
            .map_err(|e| ToolError::Execution(format!("trigger_schedule: {e}")))?;

        let result = serde_json::json!({
            "entry_id": input.entry_id,
            "title": entry.title,
            "status": "triggered",
        });
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::make_scheduler;
    use chrono::Utc;
    use ozzie_core::domain::{ScheduleEntry, ScheduleSource, TaskTemplate};

    #[tokio::test]
    async fn trigger_schedule_success() {
        let sched = make_scheduler();

        sched
            .add_entry(ScheduleEntry {
                id: "sched_trig".to_string(),
                source: ScheduleSource::Dynamic {
                    task_template: TaskTemplate {
                        title: "Trigger me".to_string(),
                        description: "test".to_string(),
                        tools: Vec::new(),
                        work_dir: None,
                        env: Default::default(),
                        approved_tools: Vec::new(),
                    },
                },
                conversation_id: None,
                title: "Trigger me".to_string(),
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
            })
            .unwrap();

        let tool = TriggerScheduleTool::new(sched.clone());
        let result = tool.run(r#"{"entry_id":"sched_trig"}"#).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "triggered");
        assert_eq!(parsed["title"], "Trigger me");

        let entry = sched.get_entry("sched_trig").unwrap();
        assert_eq!(entry.run_count, 1);
        assert!(entry.last_run_at.is_some());
    }

    #[tokio::test]
    async fn trigger_schedule_not_found() {
        let sched = make_scheduler();
        let tool = TriggerScheduleTool::new(sched);

        let result = tool.run(r#"{"entry_id":"nope"}"#).await;
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn trigger_schedule_missing_id() {
        let sched = make_scheduler();
        let tool = TriggerScheduleTool::new(sched);

        let result = tool.run(r#"{"entry_id":""}"#).await;
        assert!(result.unwrap_err().to_string().contains("required"));
    }
}
