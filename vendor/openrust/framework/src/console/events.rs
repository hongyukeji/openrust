//! Events emitted by Artisan commands and scheduled tasks.

use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::contracts::events::Event;

use super::{Application, ConsoleError, scheduling};

/// 首次装配当前应用的 Artisan 注册表时派发。
pub struct ArtisanStarting {
    artisan: Mutex<Option<Application>>,
}

impl ArtisanStarting {
    pub const NAME: &'static str = "OpenRust\\Console\\Events\\ArtisanStarting";

    /// 在同步事件监听器内配置本次即将使用的命令注册表。
    pub fn with_artisan<T>(
        &self,
        configure: impl FnOnce(&mut Application) -> Result<T, ConsoleError>,
    ) -> Result<T, ConsoleError> {
        let mut registry = self
            .artisan
            .try_lock()
            .map_err(|_| ConsoleError::new("Artisan 启动注册表正在配置。"))?;
        configure(
            registry
                .as_mut()
                .ok_or_else(|| ConsoleError::new("Artisan 启动注册表配置已结束。"))?,
        )
    }

    fn take(&self) -> Option<Application> {
        self.artisan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

impl Event for ArtisanStarting {
    fn name(&self) -> &str {
        Self::NAME
    }
}

tokio::task_local! {
    static STARTING_CONSOLES: Vec<Arc<crate::container::Container>>;
}

pub(crate) fn artisan_is_starting(app: &crate::foundation::Application) -> bool {
    let container = app.container_handle();
    STARTING_CONSOLES
        .try_with(|consoles| {
            consoles
                .iter()
                .any(|active| Arc::ptr_eq(active, &container))
        })
        .unwrap_or(false)
}

pub(crate) async fn dispatch_artisan_starting(
    app: &crate::foundation::Application,
    artisan: &mut Application,
) -> Result<(), ConsoleError> {
    use crate::contracts::events::Dispatcher as _;

    let dispatcher = app
        .make::<crate::events::Dispatcher>()
        .await
        .map_err(ConsoleError::from_source)?;
    if !dispatcher.has_listeners(ArtisanStarting::NAME) {
        return Ok(());
    }

    // 监听失败或命令取消时，原注册表仍可使用，先前被保存的事件也会关闭。
    struct Registration(Arc<ArtisanStarting>);
    impl Drop for Registration {
        fn drop(&mut self) {
            self.0.take();
        }
    }
    let registration = Registration(Arc::new(ArtisanStarting {
        artisan: Mutex::new(Some(artisan.clone())),
    }));
    let mut starting = STARTING_CONSOLES.try_with(Clone::clone).unwrap_or_default();
    starting.push(app.container_handle());
    STARTING_CONSOLES
        .scope(starting, async {
            dispatcher
                .dispatch(
                    registration.0.clone(),
                    &mut crate::events::EventContext::new(),
                )
                .await
        })
        .await
        .map_err(ConsoleError::from_source)?;
    *artisan = registration
        .0
        .take()
        .expect("Artisan 启动注册表仍处于事件作用域");
    Ok(())
}

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
