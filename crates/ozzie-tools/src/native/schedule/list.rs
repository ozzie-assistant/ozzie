use std::sync::Arc;

use chrono::{DateTime, Utc};
use ozzie_core::domain::{Tool, ToolError, ToolInfo};
use ozzie_core::domain::SchedulerPort;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{schema_for, ToolSpec};

/// Lists all active schedule entries.
pub struct ListSchedulesTool {
    scheduler: Arc<dyn SchedulerPort>,
}

impl ListSchedulesTool {
    pub fn new(scheduler: Arc<dyn SchedulerPort>) -> Self {
        Self { scheduler }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            name: "list_schedules".to_string(),
            description:
                "List all active schedule entries. Optionally filter by session ID.".to_string(),
            parameters: schema_for::<ListSchedulesInput>(),
            dangerous: false,
        }
    }
}

/// Arguments for list_schedules.
#[derive(Deserialize, Default, JsonSchema)]
struct ListSchedulesInput {
    /// Optional session ID to filter by.
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Serialize)]
struct ListScheduleEntry {
    id: String,
    source: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cron_spec: Option<String>,
    #[serde(skip_serializing_if = "is_zero")]
    interval_sec: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    on_event: Option<String>,
    enabled: bool,
    run_count: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    max_runs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_at: Option<DateTime<Utc>>,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[async_trait::async_trait]
impl Tool for ListSchedulesTool {
    fn info(&self) -> ToolInfo {
        ToolInfo::with_parameters(
            "list_schedules",
            "List all active schedule entries",
            ListSchedulesTool::spec().parameters,
        )
    }

    async fn run(&self, arguments_json: &str) -> Result<String, ToolError> {
        let input: ListSchedulesInput = if arguments_json.is_empty() {
            ListSchedulesInput::default()
        } else {
            serde_json::from_str(arguments_json).unwrap_or_default()
        };

        let entries = self.scheduler.list_entries();

        let mut out: Vec<ListScheduleEntry> = Vec::new();
        for e in &entries {
            if let Some(ref filter_sid) = input.session_id {
                match &e.session_id {
                    Some(sid) if sid == filter_sid => {}
                    _ => continue,
                }
            }

            let on_event_str = e.on_event.as_ref().map(|t| t.event_type.clone());

            out.push(ListScheduleEntry {
                id: e.id.clone(),
                source: e.source.as_str().to_string(),
                title: e.title.clone(),
                cron_spec: e.cron_spec.clone(),
                interval_sec: e.interval_sec,
                on_event: on_event_str,
                enabled: e.enabled,
                run_count: e.run_count,
                max_runs: e.max_runs,
                last_run_at: e.last_run_at,
            });
        }

        let result = serde_json::json!({
            "count": out.len(),
            "entries": out,
        });
        serde_json::to_string(&result)
            .map_err(|e| ToolError::Execution(format!("list_schedules: marshal: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::make_scheduler;
    use ozzie_core::domain::{ScheduleEntry, ScheduleSource, TaskTemplate};

    #[tokio::test]
    async fn list_schedules_empty() {
        let sched = make_scheduler();
        let tool = ListSchedulesTool::new(sched);

        let result = tool.run("{}").await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 0);
        assert!(parsed["entries"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_schedules_with_entries() {
        let sched = make_scheduler();

        sched
            .add_entry(ScheduleEntry {
                id: "sched_a".to_string(),
                source: ScheduleSource::Dynamic {
                    task_template: TaskTemplate {
                        title: "A".to_string(),
                        description: "entry a".to_string(),
                        tools: Vec::new(),
                        work_dir: None,
                        env: Default::default(),
                        approved_tools: Vec::new(),
                    },
                },
                session_id: Some("s1".to_string()),
                title: "A".to_string(),
                description: "entry a".to_string(),
                cron_spec: Some("*/5 * * * *".to_string()),
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

        sched
            .add_entry(ScheduleEntry {
                id: "sched_b".to_string(),
                source: ScheduleSource::Dynamic {
                    task_template: TaskTemplate {
                        title: "B".to_string(),
                        description: "entry b".to_string(),
                        tools: Vec::new(),
                        work_dir: None,
                        env: Default::default(),
                        approved_tools: Vec::new(),
                    },
                },
                session_id: Some("s2".to_string()),
                title: "B".to_string(),
                description: "entry b".to_string(),
                cron_spec: None,
                interval_sec: 300,
                on_event: None,
                cooldown_sec: 0,
                max_runs: 10,
                run_count: 0,
                enabled: true,
                created_at: Utc::now(),
                last_run_at: None,
            })
            .unwrap();

        let tool = ListSchedulesTool::new(sched);

        let result = tool.run("{}").await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 2);

        let result = tool.run(r#"{"session_id":"s1"}"#).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["entries"][0]["title"], "A");
    }
}
