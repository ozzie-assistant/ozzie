use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracing::info;

use ozzie_core::events::{Event, EventBus, EventSource};
use ozzie_core::domain::{ScheduleEntry, ScheduleHandler, SchedulerError, SchedulerPort};

use super::cron::CronExpr;

/// Runtime representation of a schedule entry.
struct RuntimeEntry {
    entry: ScheduleEntry,
    cron: Option<CronExpr>,
}

/// Scheduler manages cron, interval, and event-based triggers.
pub struct Scheduler {
    entries: Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    bus: Arc<dyn EventBus>,
    handler: Arc<dyn ScheduleHandler>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Scheduler {
    /// Creates a new scheduler.
    pub fn new(bus: Arc<dyn EventBus>, handler: Arc<dyn ScheduleHandler>) -> Self {
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            bus,
            handler,
            shutdown: shutdown_tx,
        }
    }

    /// Adds a schedule entry. Parses cron spec if present.
    pub fn add_entry(&self, entry: ScheduleEntry) -> Result<(), SchedulerError> {
        let cron = match &entry.cron_spec {
            Some(spec) => Some(CronExpr::parse(spec)?),
            None => None,
        };
        let id = entry.id.clone();
        let rt = RuntimeEntry { entry, cron };
        let mut entries = self.entries.lock().unwrap();
        entries.insert(id, rt);
        Ok(())
    }

    /// Removes a schedule entry by ID.
    pub fn remove_entry(&self, id: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        entries.remove(id).is_some()
    }

    /// Returns all schedule entries.
    pub fn list_entries(&self) -> Vec<ScheduleEntry> {
        let entries = self.entries.lock().unwrap();
        let mut all: Vec<ScheduleEntry> = entries.values().map(|rt| rt.entry.clone()).collect();
        all.sort_by_key(|e| e.created_at);
        all
    }

    /// Returns a schedule entry by ID, if it exists.
    pub fn get_entry(&self, id: &str) -> Option<ScheduleEntry> {
        let entries = self.entries.lock().unwrap();
        entries.get(id).map(|rt| rt.entry.clone())
    }

    /// Manually triggers a schedule entry, bypassing its trigger and cooldown.
    /// Returns the entry for the caller to handle task creation.
    pub fn trigger_entry(&self, id: &str) -> Result<ScheduleEntry, SchedulerError> {
        let mut entries = self.entries.lock().unwrap();
        let rt = entries
            .get_mut(id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;

        if !rt.entry.enabled {
            return Err(SchedulerError::Other(format!(
                "entry {id} is disabled"
            )));
        }

        let now = Utc::now();
        rt.entry.last_run_at = Some(now);
        rt.entry.run_count += 1;

        if rt.entry.max_runs > 0 && rt.entry.run_count >= rt.entry.max_runs {
            rt.entry.enabled = false;
        }

        Ok(rt.entry.clone())
    }

    /// Enables or disables an entry.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if let Some(rt) = entries.get_mut(id) {
            rt.entry.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Starts the scheduler loops (cron tick + event listener).
    /// Returns a handle that stops the scheduler when dropped.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let entries = self.entries.clone();
        let handler = self.handler.clone();
        let bus = self.bus.clone();
        let mut shutdown_rx = self.shutdown.subscribe();

        tokio::spawn(async move {
            let mut event_rx = bus.subscribe(&[]);
            let mut cron_interval = tokio::time::interval(Duration::from_secs(60));
            let mut tick_interval = tokio::time::interval(Duration::from_secs(1));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        info!("scheduler shutting down");
                        break;
                    }
                    _ = cron_interval.tick() => {
                        let now = Utc::now();
                        let triggered = check_cron(&entries, &now);
                        for (entry, trigger) in triggered {
                            let h = handler.clone();
                            tokio::spawn(async move { h.on_trigger(&entry, &trigger).await });
                        }
                    }
                    _ = tick_interval.tick() => {
                        let now = Utc::now();
                        let triggered = check_intervals(&entries, &now);
                        for (entry, trigger) in triggered {
                            let h = handler.clone();
                            tokio::spawn(async move { h.on_trigger(&entry, &trigger).await });
                        }
                    }
                    result = event_rx.recv() => {
                        if let Ok(event) = result {
                            // Skip events from the scheduler itself to avoid loops
                            if event.source == EventSource::Scheduler {
                                continue;
                            }
                            let triggered = check_events(&entries, &event);
                            for (entry, trigger) in triggered {
                                let h = handler.clone();
                                tokio::spawn(async move { h.on_trigger(&entry, &trigger).await });
                            }
                        }
                    }
                }
            }
        })
    }

    /// Stops the scheduler.
    pub fn stop(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Checks all cron entries and returns those that should fire now.
fn check_cron(
    entries: &Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    now: &DateTime<Utc>,
) -> Vec<(ScheduleEntry, String)> {
    let mut triggered = Vec::new();
    let mut lock = entries.lock().unwrap();

    for rt in lock.values_mut() {
        if !rt.entry.enabled {
            continue;
        }
        let Some(cron) = &rt.cron else { continue };
        if !cron.matches(now) {
            continue;
        }
        if !cooldown_ok(&rt.entry, now) {
            continue;
        }
        if max_runs_reached(&rt.entry) {
            continue;
        }

        rt.entry.last_run_at = Some(*now);
        rt.entry.run_count += 1;
        triggered.push((rt.entry.clone(), "cron".to_string()));

        if rt.entry.max_runs > 0 && rt.entry.run_count >= rt.entry.max_runs {
            rt.entry.enabled = false;
        }
    }

    triggered
}

/// Checks all interval-based entries.
fn check_intervals(
    entries: &Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    now: &DateTime<Utc>,
) -> Vec<(ScheduleEntry, String)> {
    let mut triggered = Vec::new();
    let mut lock = entries.lock().unwrap();

    for rt in lock.values_mut() {
        if !rt.entry.enabled || rt.entry.interval_sec == 0 {
            continue;
        }

        let interval = chrono::Duration::seconds(rt.entry.interval_sec as i64);
        let should_fire = match rt.entry.last_run_at {
            Some(last) => *now - last >= interval,
            None => true,
        };

        if !should_fire {
            continue;
        }
        if !cooldown_ok(&rt.entry, now) {
            continue;
        }
        if max_runs_reached(&rt.entry) {
            continue;
        }

        rt.entry.last_run_at = Some(*now);
        rt.entry.run_count += 1;
        triggered.push((rt.entry.clone(), "interval".to_string()));

        if rt.entry.max_runs > 0 && rt.entry.run_count >= rt.entry.max_runs {
            rt.entry.enabled = false;
        }
    }

    triggered
}

/// Checks all event-based entries.
fn check_events(
    entries: &Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    event: &Event,
) -> Vec<(ScheduleEntry, String)> {
    let mut triggered = Vec::new();
    let now = Utc::now();
    let mut lock = entries.lock().unwrap();

    for rt in lock.values_mut() {
        if !rt.entry.enabled {
            continue;
        }
        let Some(trigger) = &rt.entry.on_event else {
            continue;
        };
        if !trigger.matches(event) {
            continue;
        }
        if !cooldown_ok(&rt.entry, &now) {
            continue;
        }
        if max_runs_reached(&rt.entry) {
            continue;
        }

        rt.entry.last_run_at = Some(now);
        rt.entry.run_count += 1;
        triggered.push((rt.entry.clone(), format!("event:{}", trigger.event_type)));

        if rt.entry.max_runs > 0 && rt.entry.run_count >= rt.entry.max_runs {
            rt.entry.enabled = false;
        }
    }

    triggered
}

pub(super) fn cooldown_ok(entry: &ScheduleEntry, now: &DateTime<Utc>) -> bool {
    if entry.cooldown_sec == 0 {
        return true;
    }
    match entry.last_run_at {
        Some(last) => {
            let elapsed = (*now - last).num_seconds();
            elapsed >= entry.cooldown_sec as i64
        }
        None => true,
    }
}

pub(super) fn max_runs_reached(entry: &ScheduleEntry) -> bool {
    entry.max_runs > 0 && entry.run_count >= entry.max_runs
}

impl SchedulerPort for Scheduler {
    fn add_entry(&self, entry: ScheduleEntry) -> Result<(), SchedulerError> {
        Scheduler::add_entry(self, entry)
    }
    fn remove_entry(&self, id: &str) -> bool {
        Scheduler::remove_entry(self, id)
    }
    fn list_entries(&self) -> Vec<ScheduleEntry> {
        Scheduler::list_entries(self)
    }
    fn get_entry(&self, id: &str) -> Option<ScheduleEntry> {
        Scheduler::get_entry(self, id)
    }
    fn trigger_entry(&self, id: &str) -> Result<ScheduleEntry, SchedulerError> {
        Scheduler::trigger_entry(self, id)
    }
    fn set_enabled(&self, id: &str, enabled: bool) -> bool {
        Scheduler::set_enabled(self, id, enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use chrono::Utc;
    use ozzie_core::events::{EventPayload, EventSource, Event};
    use ozzie_core::domain::{EventTrigger, ScheduleEntry, ScheduleHandler, ScheduleSource, TaskTemplate};

    #[test]
    fn event_trigger_matches() {
        let trigger = EventTrigger {
            event_type: "schedule.created".to_string(),
            filter: HashMap::from([("entry_id".to_string(), "sched_123".to_string())]),
        };

        let event = Event::new(
            EventSource::Scheduler,
            EventPayload::ScheduleCreated {
                entry_id: "sched_123".to_string(),
                title: "Test".to_string(),
                source: "dynamic".to_string(),
            },
        );
        assert!(trigger.matches(&event));

        // Wrong entry_id
        let event2 = Event::new(
            EventSource::Scheduler,
            EventPayload::ScheduleCreated {
                entry_id: "sched_999".to_string(),
                title: "Test".to_string(),
                source: "dynamic".to_string(),
            },
        );
        assert!(!trigger.matches(&event2));

        // Wrong event type
        let event3 = Event::new(
            EventSource::Scheduler,
            EventPayload::ScheduleRemoved {
                entry_id: "sched_123".to_string(),
                title: "Test".to_string(),
            },
        );
        assert!(!trigger.matches(&event3));
    }

    #[test]
    fn scheduler_add_remove() {
        let bus = Arc::new(ozzie_core::events::Bus::new(64));
        let handler = Arc::new(NoopHandler);
        let sched = Scheduler::new(bus, handler);

        let entry = ScheduleEntry {
            id: "sched_test".to_string(),
            source: ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: "Test".to_string(),
                    description: "test schedule".to_string(),
                    tools: Vec::new(),
                    work_dir: None,
                    env: HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
            session_id: None,
            title: "Test".to_string(),
            description: "test schedule".to_string(),
            cron_spec: Some("*/5 * * * *".to_string()),
            interval_sec: 0,
            on_event: None,
            cooldown_sec: 60,
            max_runs: 0,
            run_count: 0,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        };

        sched.add_entry(entry).unwrap();
        assert_eq!(sched.list_entries().len(), 1);

        assert!(sched.remove_entry("sched_test"));
        assert_eq!(sched.list_entries().len(), 0);
    }

    #[test]
    fn scheduler_enable_disable() {
        let bus = Arc::new(ozzie_core::events::Bus::new(64));
        let handler = Arc::new(NoopHandler);
        let sched = Scheduler::new(bus, handler);

        let entry = ScheduleEntry {
            id: "sched_toggle".to_string(),
            source: ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: "Toggle".to_string(),
                    description: "test".to_string(),
                    tools: Vec::new(),
                    work_dir: None,
                    env: HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
            session_id: None,
            title: "Toggle".to_string(),
            description: "test".to_string(),
            cron_spec: None,
            interval_sec: 300,
            on_event: None,
            cooldown_sec: 0,
            max_runs: 0,
            run_count: 0,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        };

        sched.add_entry(entry).unwrap();
        sched.set_enabled("sched_toggle", false);

        let entries = sched.list_entries();
        assert!(!entries[0].enabled);
    }

    #[test]
    fn cooldown_prevents_trigger() {
        let now = Utc::now();
        let entry = ScheduleEntry {
            id: "test".to_string(),
            source: ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: "Test".to_string(),
                    description: "test".to_string(),
                    tools: Vec::new(),
                    work_dir: None,
                    env: HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
            session_id: None,
            title: "Test".to_string(),
            description: "test".to_string(),
            cron_spec: None,
            interval_sec: 0,
            on_event: None,
            cooldown_sec: 60,
            max_runs: 0,
            run_count: 0,
            enabled: true,
            created_at: now,
            last_run_at: Some(now - chrono::Duration::seconds(30)),
        };
        assert!(!cooldown_ok(&entry, &now));

        let entry2 = ScheduleEntry {
            last_run_at: Some(now - chrono::Duration::seconds(61)),
            ..entry
        };
        assert!(cooldown_ok(&entry2, &now));
    }

    #[test]
    fn max_runs_limit() {
        let entry = ScheduleEntry {
            id: "test".to_string(),
            source: ScheduleSource::Dynamic {
                task_template: TaskTemplate {
                    title: "Test".to_string(),
                    description: "test".to_string(),
                    tools: Vec::new(),
                    work_dir: None,
                    env: HashMap::new(),
                    approved_tools: Vec::new(),
                },
            },
            session_id: None,
            title: "Test".to_string(),
            description: "test".to_string(),
            cron_spec: None,
            interval_sec: 0,
            on_event: None,
            cooldown_sec: 0,
            max_runs: 3,
            run_count: 3,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        };
        assert!(max_runs_reached(&entry));

        let entry2 = ScheduleEntry {
            run_count: 2,
            ..entry
        };
        assert!(!max_runs_reached(&entry2));
    }

    struct NoopHandler;

    #[async_trait::async_trait]
    impl ScheduleHandler for NoopHandler {
        async fn on_trigger(&self, _entry: &ScheduleEntry, _trigger: &str) {}
    }
}
