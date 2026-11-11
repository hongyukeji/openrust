use std::{cell::RefCell, ffi::OsString, time::SystemTime};

use crate::{
    console::{ConsoleError, ConsoleFuture, Output},
    foundation::Application,
    queue::{DispatchOptions, Payload},
};

use super::{CallbackEvent, Event, event::Action};

enum Registration {
    Event(Event),
    Callback(CallbackEvent),
}

impl Registration {
    fn event(&self) -> &Event {
        match self {
            Self::Event(event) => event,
            Self::Callback(event) => event,
        }
    }
}

/// Registers tasks and selects events due at a given time.
pub struct Schedule {
    events: Vec<Registration>,
    pub(super) store: Option<String>,
    pub(super) clock: Box<dyn Fn() -> SystemTime + Send + Sync>,
}

tokio::task_local! {
    static REGISTRATION: RefCell<Schedule>;
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            store: None,
            clock: Box::new(SystemTime::now),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's scheduling API")]
impl Schedule {
    pub fn call(
        &mut self,
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut CallbackEvent {
        self.callback(Action::Callback(Box::new(callback)))
    }

    pub fn command(
        &mut self,
        command: impl Into<OsString>,
        args: impl IntoIterator<Item = OsString>,
    ) -> &mut Event {
        self.event(Action::Command(
            std::iter::once(command.into()).chain(args).collect(),
        ))
    }

    pub fn exec(
        &mut self,
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = OsString>,
    ) -> &mut Event {
        self.event(Action::Exec(
            std::iter::once(program.into()).chain(args).collect(),
        ))
    }

    pub fn job(
        &mut self,
        payload: Payload,
        queue: Option<&str>,
        connection: Option<&str>,
    ) -> &mut CallbackEvent {
        self.callback(Action::Job(
            Box::new(payload),
            DispatchOptions {
                queue: queue.map(str::to_owned),
                connection: connection.map(str::to_owned),
                ..DispatchOptions::default()
            },
        ))
    }

    pub fn useCache(&mut self, store: impl Into<String>) {
        self.store = Some(store.into());
    }

    pub fn useClock(&mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) {
        self.clock = Box::new(clock);
    }

    fn callback(&mut self, action: Action) -> &mut CallbackEvent {
        self.events
            .push(Registration::Callback(CallbackEvent(Event::new(action))));
        let Some(Registration::Callback(event)) = self.events.last_mut() else {
            unreachable!("just inserted a callback");
        };
        event
    }

    fn event(&mut self, action: Action) -> &mut Event {
        self.events.push(Registration::Event(Event::new(action)));
        let Some(Registration::Event(event)) = self.events.last_mut() else {
            unreachable!("just inserted an event");
        };
        event
    }

    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.events.iter().map(Registration::event)
    }

    pub(crate) fn collect(
        register: impl FnOnce() -> Result<(), ConsoleError>,
    ) -> Result<Self, ConsoleError> {
        REGISTRATION.sync_scope(RefCell::new(Self::default()), || {
            register()?;
            let schedule = REGISTRATION.with(|current| current.take());
            for event in schedule.events() {
                if matches!(event.action, Action::Callback(_) | Action::Job(..))
                    && (event.one_server || event.overlap.is_some())
                    && event.description.as_deref().is_none_or(|name| name.trim().is_empty())
                {
                    return Err(ConsoleError::new("Give scheduled callbacks and jobs a name before enabling a scheduling mutex."));
                }
            }
            Ok(schedule)
        })
    }
}

/// Accesses the schedule being configured by the current Artisan application.
pub struct ScheduleFacade;

impl ScheduleFacade {
    pub fn configure(
        register: impl FnOnce(&mut Schedule) -> Result<(), ConsoleError>,
    ) -> Result<(), ConsoleError> {
        REGISTRATION
            .try_with(|schedule| {
                let mut schedule = schedule
                    .try_borrow_mut()
                    .map_err(|_| ConsoleError::new("Schedule is already being configured."))?;
                register(&mut schedule)
            })
            .map_err(|_| {
                ConsoleError::new("Schedule registration requires an Artisan application scope.")
            })?
    }
}
