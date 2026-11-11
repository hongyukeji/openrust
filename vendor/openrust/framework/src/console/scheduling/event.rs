use std::{ffi::OsString, fmt::Write, time::Duration};

use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};

use crate::{
    console::{ConsoleError, ConsoleFuture, Output},
    foundation::Application,
    queue::{DispatchOptions, Payload},
};

type Callback = dyn for<'a> Fn(&'a Application, &'a mut Output) -> ConsoleFuture<'a, Result<(), ConsoleError>>
    + Send
    + Sync;
type Condition = dyn Fn(&Application) -> bool + Send + Sync;

pub(super) enum Action {
    Callback(Box<Callback>),
    Command(Vec<OsString>),
    Exec(Vec<OsString>),
    Job(Box<Payload>, DispatchOptions),
}

/// A scheduled task with a frequency, time zone, and execution constraints.
pub struct Event {
    pub(super) action: Action,
    pub(super) expression: Cron,
    pub(super) cron_text: String,
    pub(super) zone: Option<Tz>,
    pub(super) interval: Option<Duration>,
    pub(super) overlap: Option<Duration>,
    pub(super) one_server: bool,
    pub(super) description: Option<String>,
    environments: Vec<String>,
    filters: Vec<Box<Condition>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's scheduling API")]
impl Event {
    pub(super) fn new(action: Action) -> Self {
        Self {
            action,
            expression: "* * * * *".parse().expect("valid default cron"),
            cron_text: "* * * * *".into(),
            zone: None,
            interval: None,
            overlap: None,
            one_server: false,
            description: None,
            environments: Vec::new(),
            filters: Vec::new(),
        }
    }

    pub fn cron(&mut self, expression: &str) -> Result<&mut Self, ConsoleError> {
        self.expression = CronParser::builder()
            .seconds(Seconds::Disallowed)
            .year(Year::Disallowed)
            .sloppy_ranges(true)
            .build()
            .parse(expression)
            .map_err(super::error)?;
        self.cron_text = expression.into();
        self.interval = None;
        Ok(self)
    }

    pub fn everyMinute(&mut self) -> &mut Self {
        self.cron("* * * * *").expect("valid minute frequency")
    }

    pub fn dailyAt(&mut self, time: &str) -> Result<&mut Self, ConsoleError> {
        let time = chrono::NaiveTime::parse_from_str(time, "%H:%M").map_err(super::error)?;
        self.cron(&format!("{} {} * * *", time.minute(), time.hour()))
    }

    pub fn everySeconds(&mut self, seconds: u32) -> Result<&mut Self, ConsoleError> {
        if seconds == 0 || seconds >= 60 || !60_u32.is_multiple_of(seconds) {
            return Err(ConsoleError::new(
                "A repeat interval must divide one minute and be less than 60 seconds.",
            ));
        }
        self.everyMinute();
        self.interval = Some(Duration::from_secs(seconds.into()));
        Ok(self)
    }

    pub fn everySecond(&mut self) -> &mut Self {
        self.repeat(1)
    }
    pub fn everyTwoSeconds(&mut self) -> &mut Self {
        self.repeat(2)
    }
    pub fn everyFiveSeconds(&mut self) -> &mut Self {
        self.repeat(5)
    }
    pub fn everyTenSeconds(&mut self) -> &mut Self {
        self.repeat(10)
    }
    pub fn everyFifteenSeconds(&mut self) -> &mut Self {
        self.repeat(15)
    }
    pub fn everyTwentySeconds(&mut self) -> &mut Self {
        self.repeat(20)
    }
    pub fn everyThirtySeconds(&mut self) -> &mut Self {
        self.repeat(30)
    }

    fn repeat(&mut self, seconds: u32) -> &mut Self {
        self.everySeconds(seconds).expect("valid repeat frequency")
    }

    pub fn timezone(&mut self, zone: Tz) -> &mut Self {
        self.zone = Some(zone);
        self
    }

    pub fn environments(&mut self, names: &[&str]) -> &mut Self {
        self.environments = names.iter().map(|name| (*name).into()).collect();
        self
    }

    pub fn when(
        &mut self,
        condition: impl Fn(&Application) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.filters.push(Box::new(condition));
        self
    }

    pub fn skip(
        &mut self,
        condition: impl Fn(&Application) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.when(move |app| !condition(app))
    }

    pub fn name(&mut self, name: impl Into<String>) -> &mut Self {
        self.description = Some(name.into());
        self
    }

    pub fn withoutOverlapping(&mut self) -> &mut Self {
        self.overlap = Some(Duration::from_secs(1440 * 60));
        self
    }

    pub fn withoutOverlappingFor(&mut self, minutes: u32) -> Result<&mut Self, ConsoleError> {
        if minutes == 0 {
            return Err(ConsoleError::new(
                "An overlap lock requires a positive expiry in minutes.",
            ));
        }
        self.overlap = Some(Duration::from_secs(u64::from(minutes) * 60));
        Ok(self)
    }

    pub fn onOneServer(&mut self) -> &mut Self {
        self.one_server = true;
        self
    }

    pub(super) fn due(&self, now: DateTime<Utc>, zone: Tz) -> Result<bool, ConsoleError> {
        let minute = now.with_second(0).expect("valid second");
        self.expression
            .is_time_matching(&minute.with_timezone(&self.zone.unwrap_or(zone)))
            .map_err(super::error)
    }

    pub(super) fn allowed(&self, app: &Application, environment: &str) -> bool {
        (self.environments.is_empty() || self.environments.iter().any(|name| name == environment))
            && self.filters.iter().all(|condition| condition(app))
    }

    pub(super) fn label(&self) -> String {
        self.description
            .clone()
            .unwrap_or_else(|| match &self.action {
                Action::Callback(_) => "Closure".into(),
                Action::Command(args) | Action::Exec(args) => args
                    .iter()
                    .map(|arg| arg.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
                Action::Job(payload, _) => payload.name().into(),
            })
    }

    pub(super) fn mutex_key(&self) -> String {
        let mut digest = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
        let mut field = |bytes: &[u8]| {
            digest.update(&bytes.len().to_be_bytes());
            digest.update(bytes);
        };
        field(self.cron_text.as_bytes());
        match &self.action {
            Action::Callback(_) => field(b"callback"),
            Action::Command(_) => field(b"command"),
            Action::Exec(_) => field(b"exec"),
            Action::Job(payload, options) => {
                field(b"job");
                field(payload.name().as_bytes());
                field(options.connection.as_deref().unwrap_or_default().as_bytes());
                field(options.queue.as_deref().unwrap_or_default().as_bytes());
            }
        }
        if let Action::Command(args) | Action::Exec(args) = &self.action {
            for arg in args {
                field(arg.as_encoded_bytes());
            }
        }
        if let Some(name) = &self.description {
            field(name.as_bytes());
        }
        digest
            .finish()
            .as_ref()
            .iter()
            .fold(String::from("schedule:"), |mut key, byte| {
                write!(&mut key, "{byte:02x}").expect("writing to a String");
                key
            })
    }
}
