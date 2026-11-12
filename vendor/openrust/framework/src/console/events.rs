//! Events emitted by Artisan commands and scheduled tasks.

use std::{error::Error, sync::Arc, time::Duration};

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

#[derive(Clone)]
pub struct ScheduledTaskStarting {
    pub task: Arc<scheduling::Event>,
}

impl Event for ScheduledTaskStarting {
    fn name(&self) -> &str {
        "console.scheduled_task_starting"
    }
}

#[derive(Clone)]
pub struct ScheduledTaskFinished {
    pub task: Arc<scheduling::Event>,
    pub runtime: Duration,
}

impl Event for ScheduledTaskFinished {
    fn name(&self) -> &str {
        "console.scheduled_task_finished"
    }
}

#[derive(Clone)]
pub struct ScheduledTaskFailed {
    pub task: Arc<scheduling::Event>,
    pub exception: Arc<dyn Error + Send + Sync>,
    pub runtime: Duration,
}

impl Event for ScheduledTaskFailed {
    fn name(&self) -> &str {
        "console.scheduled_task_failed"
    }
}
