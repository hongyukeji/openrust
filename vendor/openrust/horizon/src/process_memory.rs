use crate::{MasterSupervisorLooped, SupervisorLooped};
use openrust::{
    contracts::{
        container::BindingResolutionError,
        events::{Event, Listener},
    },
    events::{EventContext, EventFuture, EventResult, ListenerResponse},
    support::{
        facades,
        facades::{Config, Facade},
    },
};
use std::sync::Mutex;

pub(crate) fn memory_usage() -> f64 {
    memory_stats::memory_stats().map_or(0., |usage| usage.physical_mem as f64 / 1024. / 1024.)
}

#[derive(Default)]
pub(crate) struct MemoryCheck(Mutex<Option<bool>>);

impl MemoryCheck {
    pub(crate) fn exceeded(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .unwrap_or(false)
    }

    fn evaluate(
        &self,
        limit: impl FnOnce() -> Result<u64, BindingResolutionError>,
    ) -> Result<Option<(f64, u64)>, BindingResolutionError> {
        let mut checked = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if checked.is_some() {
            return Ok(None);
        }
        let limit = limit()?;
        let usage = memory_usage();
        let exceeded = usage > limit as f64;
        *checked = Some(exceeded);
        Ok(exceeded.then_some((usage, limit)))
    }
}

macro_rules! memory_event {
    ($name:ident) => {
        /// Memory usage in MiB immediately before a memory-limit exit.
        pub struct $name {
            pub name: String,
            pub memory_usage: f64,
            pub memory_limit: u64,
        }

        impl $name {
            pub const NAME: &str = stringify!($name);
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}

memory_event!(MasterSupervisorOutOfMemory);
memory_event!(SupervisorOutOfMemory);

impl SupervisorOutOfMemory {
    pub fn get_memory_usage(&self) -> f64 {
        self.memory_usage
    }

    pub fn set_memory_usage(&mut self, memory_usage: f64) -> &mut Self {
        self.memory_usage = memory_usage;
        self
    }
}

async fn dispatch(event: impl Event + 'static) {
    if let Err(error) = facades::Event::dispatch(event).await {
        let _ = facades::Log::warning(
            &format!("Horizon memory-limit listener failed: {error}"),
            &Default::default(),
        );
    }
}

macro_rules! monitor_listener {
    ($listener:ident, $looped:ident, $exceeded:ident, $limit:expr) => {
        pub struct $listener;

        impl $listener {
            pub(crate) async fn check(event: &$looped) -> EventResult<()> {
                if let Some((memory_usage, memory_limit)) =
                    event.memory_check.evaluate(|| ($limit)(event))?
                {
                    dispatch($exceeded {
                        name: event.name.clone(),
                        memory_usage,
                        memory_limit,
                    })
                    .await;
                }
                Ok(())
            }
        }

        impl Listener for $listener {
            fn handle<'a>(
                &'a self,
                event: &'a dyn Event,
                _: &'a mut EventContext<'_>,
            ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
                Box::pin(async move {
                    if let Some(event) = event.downcast_ref::<$looped>() {
                        Self::check(event).await?;
                        if event.memory_check.exceeded() {
                            return Ok(Some(ListenerResponse::Stop));
                        }
                    }
                    Ok(None)
                })
            }
        }
    };
}

monitor_listener!(
    MonitorMasterSupervisorMemory,
    MasterSupervisorLooped,
    MasterSupervisorOutOfMemory,
    |_: &MasterSupervisorLooped| Ok(Config::get_facade_root()?
        .get("horizon.memory_limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(64))
);
monitor_listener!(
    MonitorSupervisorMemory,
    SupervisorLooped,
    SupervisorOutOfMemory,
    |event: &SupervisorLooped| Ok(event.memory_limit)
);
