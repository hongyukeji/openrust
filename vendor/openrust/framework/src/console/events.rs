//! Events emitted by Artisan commands and scheduled tasks.

use std::sync::Arc;

use crate::contracts::events::Event;

use super::scheduling;

#[derive(Clone, Copy, Debug)]
pub struct SchedulePaused;

impl Event for SchedulePaused {
    fn name(&self) -> &str {
        "console.schedule_paused"
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ScheduleResumed;

impl Event for ScheduleResumed {
    fn name(&self) -> &str {
        "console.schedule_resumed"
    }
}

#[derive(Clone)]
pub struct ScheduledTaskSkipped {
    pub task: Arc<scheduling::Event>,
}

impl Event for ScheduledTaskSkipped {
    fn name(&self) -> &str {
        "console.scheduled_task_skipped"
    }
}
