//! Artisan schedules, due events, and scheduling mutexes.

mod cache_event_mutex;
mod cache_scheduling_mutex;
mod callback_event;
mod event;
mod event_mutex;
mod pending_event_attributes;
mod schedule;
mod schedule_run_command;
mod scheduling_mutex;
mod task_output;

pub use cache_event_mutex::CacheEventMutex;
pub use cache_scheduling_mutex::CacheSchedulingMutex;
pub use callback_event::CallbackEvent;
pub use event::Event;
pub use event_mutex::EventMutex;
pub use schedule::{Schedule, ScheduleFacade};
pub use schedule_run_command::ScheduleRunCommand;
pub(crate) use schedule_run_command::register;
pub use scheduling_mutex::SchedulingMutex;

fn error(error: impl std::fmt::Display) -> crate::console::ConsoleError {
    crate::console::ConsoleError::new(error.to_string())
}
