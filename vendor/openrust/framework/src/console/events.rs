//! Events emitted by Artisan commands and scheduled tasks.

use std::{error::Error, sync::Arc, time::Duration};

use crate::contracts::events::Event;

use super::scheduling;

/// A parsed Artisan command is about to execute in its native execution scope.
pub struct CommandStarting {
    pub command: String,
    pub input: clap::ArgMatches,
    pub definition: Arc<clap::Command>,
}
impl CommandStarting {
    pub const NAME: &'static str = "OpenRust\\Console\\Events\\CommandStarting";
}
impl Event for CommandStarting {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// A command returned an exit code. Duration excludes event listeners.
pub struct CommandFinished {
    pub command: String,
    pub input: clap::ArgMatches,
    pub definition: Arc<clap::Command>,
    pub exit_code: i32,
    pub duration: Duration,
    pub output: String,
}
impl CommandFinished {
    pub const NAME: &'static str = "OpenRust\\Console\\Events\\CommandFinished";
}
impl Event for CommandFinished {
    fn name(&self) -> &str {
        Self::NAME
    }
}

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
    pub runtime: Option<Duration>,
}

impl Event for ScheduledTaskFailed {
    fn name(&self) -> &str {
        "console.scheduled_task_failed"
    }
}

/// Fired after a background scheduled command has finished and released its overlap lock.
#[derive(Clone)]
pub struct ScheduledBackgroundTaskFinished {
    pub task: Arc<scheduling::Event>,
    pub exit_code: i32,
}

impl Event for ScheduledBackgroundTaskFinished {
    fn name(&self) -> &str {
        "console.scheduled_background_task_finished"
    }
}

/// A scheduled execution has run its after hooks, including unsuccessful callbacks.
pub struct ScheduledTaskCompleted {
    pub command: String,
    pub description: Option<String>,
    pub expression: String,
    pub timezone: Option<String>,
    pub output: Option<std::path::PathBuf>,
    pub should_append_output: bool,
}
impl Event for ScheduledTaskCompleted {
    fn name(&self) -> &str {
        "console.scheduled_task_completed"
    }
}
