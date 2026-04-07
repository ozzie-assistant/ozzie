use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use ozzie_core::domain::{Tool, ToolError, ToolInfo, TOOL_CTX};
use ozzie_core::events::{EventBus, EventPayload, EventSource};
use ozzie_core::domain::{EventTrigger, ScheduleEntry, ScheduleSource, SchedulerPort, TaskTemplate};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::registry::{schema_for, ToolSpec};

/// Creates a recurring scheduled task.
pub struct ScheduleTaskTool {
    scheduler: Arc<dyn SchedulerPort>,
    bus: Arc<dyn EventBus>,
}

impl ScheduleTaskTool {
    pub fn new(scheduler: Arc<dyn SchedulerPort>, bus: Arc<dyn EventBus>) -> Self {
        Self { scheduler, bus }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "schedule_task".to_string(),
            description: "Create a recurring scheduled task that runs on a cron schedule, at a fixed interval, or in response to an event. Returns the schedule entry ID.".to_string(),
            parameters: schema_for::<ScheduleTaskInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for schedule_task.
#[derive(Deserialize, JsonSchema)]
struct ScheduleTaskInput {
    /// Short title for the schedule.
    title: String,
    /// Detailed description of what the recurring task should do.
    description: String,
    /// 5-field cron expression (e.g. "*/5 * * * *"). Mutually exclusive with interval_sec and on_event.
    #[serde(default)]
    cron: Option<String>,
    /// Fixed interval in seconds (minimum 5). Mutually exclusive with cron and on_event.
    #[serde(default)]
    interval_sec: Option<u64>,
    /// Event type to trigger on (e.g. "task.completed"). Mutually exclusive with cron and interval_sec.
    #[serde(default)]
    on_event: Option<String>,
    /// Tool names the scheduled task agent can use.
    #[serde(default)]
    tools: Vec<String>,
    /// Working directory for the task.
    #[serde(default)]
    work_dir: Option<String>,
    /// Minimum seconds between triggers (default 60).
    #[serde(default)]
    cooldown_sec: Option<u64>,
    /// Maximum number of runs before auto-disabling (0 = unlimited).
    #[serde(default)]
    max_runs: Option<u32>,
}

#[async_trait::async_trait]
impl Tool for ScheduleTaskTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "schedule_task",
            "Create a recurring scheduled task",
            ScheduleTaskTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: ScheduleTaskInput = serde_json::from_str(arguments_json)
            .map_err(|e| ToolError::Execution(format!("schedule_task: parse input: {e}")))?;

        if input.title.is_empty() {
            return Err(ToolError::Execution(
                "schedule_task: title is required".to_string(),
            ));
        }
        if input.description.is_empty() {
            return Err(ToolError::Execution(
                "schedule_task: description is required".to_string(),
            ));
        }

        let trigger_count = input.cron.is_some() as u8
            + input.interval_sec.is_some() as u8
            + input.on_event.is_some() as u8;

        if trigger_count == 0 {
            return Err(ToolError::Execution(
                "schedule_task: one of cron, interval_sec, or on_event is required. Example: {\"cron\": \"0 12 * * *\"} for daily at noon, {\"interval_sec\": 3600} for every hour, {\"on_event\": \"task.completed\"} for event-driven".to_string(),
            ));
        }
        if trigger_count > 1 {
            return Err(ToolError::Execution(
                "schedule_task: cron, interval_sec, and on_event are mutually exclusive"
                    .to_string(),
            ));
        }

        if let Some(sec) = input.interval_sec
            && sec < 5
        {
            return Err(ToolError::Execution(
                "schedule_task: interval_sec must be at least 5".to_string(),
            ));
        }

        let on_event = input.on_event.as_ref().map(|event_str| {
            EventTrigger {
                event_type: event_str.clone(),
                filter: HashMap::new(),
            }
        });

        let session_id = TOOL_CTX
            .try_with(|ctx| ctx.session_id.clone())
            .ok()
            .filter(|s| !s.is_empty());

        let entry_id = format!("sched_{}", Utc::now().timestamp_millis());

        let entry = ScheduleEntry {
            id: entry_id.clone(),
            source: ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: input.title.clone(),
                    description: input.description.clone(),
                    tools: input.tools,
                    work_dir: input.work_dir,
                    env: HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
            session_id,
            title: input.title.clone(),
            description: input.description.clone(),
            cron_spec: input.cron.clone(),
            interval_sec: input.interval_sec.unwrap_or(0),
            on_event,
            cooldown_sec: input.cooldown_sec.unwrap_or(60),
            max_runs: input.max_runs.unwrap_or(0),
            run_count: 0,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        };

        self.scheduler
            .add_entry(entry)
            .map_err(|e| ToolError::Execution(format!("schedule_task: {e}")))?;

        self.bus.publish(ozzie_core::events::Event::new(
            EventSource::Scheduler,
            EventPayload::ScheduleCreated {
                entry_id: entry_id.clone(),
                title: input.title.clone(),
                source: "dynamic".to_string(),
            },
        ));

        let result = serde_json::json!({
            "entry_id": entry_id,
            "status": "created",
            "title": input.title,
        });
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::{make_bus, make_scheduler};
    use ozzie_core::domain::ToolContext;

    async fn with_session<F, Fut>(session_id: &str, f: F) -> Result<String, ToolError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<String, ToolError>>,
    {
        let ctx = ToolContext {
            session_id: session_id.to_string(),
            ..Default::default()
        };
        TOOL_CTX.scope(ctx, f()).await
    }

    #[tokio::test]
    async fn schedule_task_with_cron() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched.clone(), bus);

        let result = with_session("s1", || {
            tool.run(
                r#"{"title":"Daily check","description":"Run daily health check","cron":"0 12 * * *"}"#,
            )
        })
        .await
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "created");

        let entries = sched.list_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Daily check");
        assert_eq!(entries[0].cron_spec, Some("0 12 * * *".to_string()));
    }

    #[tokio::test]
    async fn schedule_task_with_interval() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched.clone(), bus);

        let result = tool
            .run(r#"{"title":"Periodic","description":"Every 5 min","interval_sec":300}"#)
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "created");
        assert_eq!(sched.list_entries()[0].interval_sec, 300);
    }

    #[tokio::test]
    async fn schedule_task_with_event() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched.clone(), bus);

        let result = tool
            .run(r#"{"title":"On task done","description":"React to task completion","on_event":"task.completed"}"#)
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["status"], "created");
        assert!(sched.list_entries()[0].on_event.is_some());
    }

    #[tokio::test]
    async fn schedule_task_missing_title() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched, bus);

        let result = tool
            .run(r#"{"title":"","description":"desc","cron":"0 * * * *"}"#)
            .await;
        assert!(result.unwrap_err().to_string().contains("title is required"));
    }

    #[tokio::test]
    async fn schedule_task_no_trigger() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched, bus);

        let result = tool
            .run(r#"{"title":"Test","description":"No trigger"}"#)
            .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("one of cron, interval_sec, or on_event"));
    }

    #[tokio::test]
    async fn schedule_task_multiple_triggers() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched, bus);

        let result = tool
            .run(r#"{"title":"Test","description":"Conflicting","cron":"* * * * *","interval_sec":60}"#)
            .await;
        assert!(result.unwrap_err().to_string().contains("mutually exclusive"));
    }

    #[tokio::test]
    async fn schedule_task_interval_too_small() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched, bus);

        let result = tool
            .run(r#"{"title":"Test","description":"Too fast","interval_sec":2}"#)
            .await;
        assert!(result.unwrap_err().to_string().contains("at least 5"));
    }

    #[tokio::test]
    async fn schedule_task_invalid_json() {
        let sched = make_scheduler();
        let bus = make_bus();
        let tool = ScheduleTaskTool::new(sched, bus);

        let result = tool.run("not json").await;
        assert!(result.is_err());
    }
}
