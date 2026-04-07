use std::sync::Arc;

use ozzie_core::domain::{ScheduleEntry, ScheduleHandler};
use ozzie_core::events::EventBus;
use ozzie_runtime::scheduler::Scheduler;

pub fn make_scheduler() -> Arc<Scheduler> {
    let bus = Arc::new(ozzie_core::events::Bus::new(64));
    let handler = Arc::new(NoopHandler);
    Arc::new(Scheduler::new(bus, handler))
}

pub fn make_bus() -> Arc<dyn EventBus> {
    Arc::new(ozzie_core::events::Bus::new(64))
}

struct NoopHandler;

#[async_trait::async_trait]
impl ScheduleHandler for NoopHandler {
    async fn on_trigger(&self, _entry: &ScheduleEntry, _trigger: &str) {}
}
