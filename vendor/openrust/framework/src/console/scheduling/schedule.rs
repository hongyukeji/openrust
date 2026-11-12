use std::{cell::RefCell, ffi::OsString, sync::Arc, time::SystemTime};

use crate::{
    console::{ConsoleError, ConsoleFuture, Output},
    foundation::Application,
    queue::{DispatchOptions, Payload},
};

use super::{
    CallbackEvent, Event,
    event::{Action, daily_expression, parse_cron, repeat_interval},
    pending_event_attributes::PendingEventAttributes,
};

enum Registration {
    Event(Arc<Event>),
    Callback(CallbackEvent),
}

impl Registration {
    fn event(&self) -> &Arc<Event> {
        match self {
            Self::Event(event) => event,
            Self::Callback(event) => &event.0,
        }
    }
}

/// Registers tasks and selects events due at a given time.
pub struct Schedule {
    events: Vec<Registration>,
    pending_attributes: Option<PendingEventAttributes>,
    groups: Vec<PendingEventAttributes>,
    pub(super) store: Option<String>,
    pub(super) pausable: bool,
    pub(super) clock: Box<dyn Fn() -> SystemTime + Send + Sync>,
}

tokio::task_local! {
    static REGISTRATION: RefCell<Schedule>;
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            pending_attributes: None,
            groups: Vec::new(),
            store: None,
            pausable: true,
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

    /// Disables pause controls for this application's scheduler.
    pub fn doNotAllowPausing(&mut self) {
        self.pausable = false;
    }

    pub fn useCache(&mut self, store: impl Into<String>) {
        self.store = Some(store.into());
    }

    pub fn useClock(&mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) {
        self.clock = Box::new(clock);
    }

    /// Applies the pending scheduling attributes to tasks registered in this group.
    pub fn group(
        &mut self,
        register: impl FnOnce(&mut Self) -> Result<(), ConsoleError>,
    ) -> Result<(), ConsoleError> {
        let attributes = self.pending_attributes.take().ok_or_else(|| {
            ConsoleError::new("Invoke a scheduling attribute before defining a schedule group.")
        })?;
        self.groups.push(attributes);
        let result = register(self);
        self.pending_attributes = None;
        self.groups.pop();
        result
    }

    pub fn cron(&mut self, expression: &str) -> Result<&mut Self, ConsoleError> {
        let cron = parse_cron(expression)?;
        let expression = expression.to_owned();
        Ok(self.with_pending(move |event| {
            event.set_frequency(cron.clone(), &expression, None);
        }))
    }

    pub fn everyMinute(&mut self) -> &mut Self {
        self.cron("* * * * *").expect("valid minute frequency")
    }

    pub fn daily(&mut self) -> &mut Self {
        self.cron("0 0 * * *").expect("valid daily frequency")
    }

    pub fn twiceDaily(&mut self) -> &mut Self {
        self.cron("0 1,13 * * *")
            .expect("valid twice-daily frequency")
    }

    pub fn dailyAt(&mut self, time: &str) -> Result<&mut Self, ConsoleError> {
        self.cron(&daily_expression(time)?)
    }

    pub fn everySeconds(&mut self, seconds: u32) -> Result<&mut Self, ConsoleError> {
        let interval = repeat_interval(seconds)?;
        let cron = parse_cron("* * * * *").expect("valid minute frequency");
        Ok(self.with_pending(move |event| {
            event.set_frequency(cron.clone(), "* * * * *", Some(interval));
        }))
    }

    pub fn everySecond(&mut self) -> &mut Self {
        self.everySeconds(1).expect("valid repeat frequency")
    }

    pub fn everyTwoSeconds(&mut self) -> &mut Self {
        self.everySeconds(2).expect("valid repeat frequency")
    }

    pub fn everyFiveSeconds(&mut self) -> &mut Self {
        self.everySeconds(5).expect("valid repeat frequency")
    }

    pub fn everyTenSeconds(&mut self) -> &mut Self {
        self.everySeconds(10).expect("valid repeat frequency")
    }

    pub fn everyFifteenSeconds(&mut self) -> &mut Self {
        self.everySeconds(15).expect("valid repeat frequency")
    }

    pub fn everyTwentySeconds(&mut self) -> &mut Self {
        self.everySeconds(20).expect("valid repeat frequency")
    }

    pub fn everyThirtySeconds(&mut self) -> &mut Self {
        self.everySeconds(30).expect("valid repeat frequency")
    }

    pub fn timezone(&mut self, zone: chrono_tz::Tz) -> &mut Self {
        self.with_pending(move |event| {
            event.timezone(zone);
        })
    }

    pub fn onOneServer(&mut self) -> &mut Self {
        self.with_pending(|event| {
            event.onOneServer();
        })
    }

    pub fn withoutOverlapping(&mut self) -> &mut Self {
        self.with_pending(|event| {
            event.withoutOverlapping();
        })
    }

    pub fn evenInMaintenanceMode(&mut self) -> &mut Self {
        self.with_pending(|event| {
            event.evenInMaintenanceMode();
        })
    }

    pub fn evenWhenPaused(&mut self) -> &mut Self {
        self.with_pending(|event| {
            event.evenWhenPaused();
        })
    }

    pub fn withoutOverlappingFor(&mut self, minutes: u32) -> Result<&mut Self, ConsoleError> {
        if minutes == 0 {
            return Err(ConsoleError::new(
                "An overlap lock requires a positive expiry in minutes.",
            ));
        }
        Ok(self.with_pending(move |event| {
            event
                .withoutOverlappingFor(minutes)
                .expect("validated overlap expiry");
        }))
    }

    pub fn environments(&mut self, names: &[&str]) -> &mut Self {
        let names: Vec<_> = names.iter().map(|name| (*name).to_owned()).collect();
        self.with_pending(move |event| {
            event.environments(&names.iter().map(String::as_str).collect::<Vec<_>>());
        })
    }

    pub fn when(
        &mut self,
        condition: impl Fn(&Application) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        let condition = Arc::new(condition);
        self.with_pending(move |event| {
            let condition = condition.clone();
            event.when(move |app| condition(app));
        })
    }

    pub fn skip(
        &mut self,
        condition: impl Fn(&Application) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.when(move |app| !condition(app))
    }

    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        let name = name.into();
        self.with_pending(move |event| {
            event.name(name.clone());
        })
    }

    pub fn withAttributes(
        &mut self,
        attributes: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> &mut Self {
        let attributes: Vec<_> = attributes.into_iter().collect();
        self.with_pending(move |event| {
            event.withAttributes(attributes.iter().cloned());
        })
    }

    fn with_pending(
        &mut self,
        configure: impl Fn(&mut Event) + Send + Sync + 'static,
    ) -> &mut Self {
        self.pending_attributes
            .get_or_insert_with(|| self.groups.last().cloned().unwrap_or_default())
            .push(configure);
        self
    }

    fn new_event(&mut self, action: Action) -> Arc<Event> {
        let mut event = Event::new(action);
        if let Some(attributes) = self.pending_attributes.take() {
            attributes.apply(&mut event);
        } else if let Some(attributes) = self.groups.last() {
            attributes.apply(&mut event);
        }
        Arc::new(event)
    }

    fn callback(&mut self, action: Action) -> &mut CallbackEvent {
        let event = self.new_event(action);
        self.events
            .push(Registration::Callback(CallbackEvent(event)));
        let Some(Registration::Callback(event)) = self.events.last_mut() else {
            unreachable!("just inserted a callback");
        };
        event
    }

    fn event(&mut self, action: Action) -> &mut Event {
        let event = self.new_event(action);
        self.events.push(Registration::Event(event));
        let Some(Registration::Event(event)) = self.events.last_mut() else {
            unreachable!("just inserted an event");
        };
        Arc::get_mut(event).expect("scheduled event is mutable during registration")
    }

    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.event_handles().map(Arc::as_ref)
    }

    pub(super) fn event_handles(&self) -> impl Iterator<Item = &Arc<Event>> {
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
