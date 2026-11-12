use std::{ffi::OsString, fmt::Write, time::Duration};

use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};
use serde_json::{Map, Value};

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
    even_in_maintenance_mode: bool,
    even_when_paused: bool,
    environments: Vec<String>,
    filters: Vec<Box<Condition>>,
    attributes: Map<String, Value>,
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
            even_in_maintenance_mode: false,
            even_when_paused: false,
            environments: Vec::new(),
            filters: Vec::new(),
            attributes: Map::new(),
        }
    }

    pub fn cron(&mut self, expression: &str) -> Result<&mut Self, ConsoleError> {
        let cron = parse_cron(expression)?;
        self.set_frequency(cron, expression, None);
        Ok(self)
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
        self.everyMinute();
        self.interval = Some(interval);
        Ok(self)
    }

    pub(super) fn set_frequency(&mut self, cron: Cron, text: &str, interval: Option<Duration>) {
        self.expression = cron;
        self.cron_text = text.into();
        self.interval = interval;
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

    /// Allows this event to run while the application is in maintenance mode.
    pub fn evenInMaintenanceMode(&mut self) -> &mut Self {
        self.even_in_maintenance_mode = true;
        self
    }

    pub fn runsInMaintenanceMode(&self) -> bool {
        self.even_in_maintenance_mode
    }

    /// Allows this event to run while the scheduler is paused.
    pub fn evenWhenPaused(&mut self) -> &mut Self {
        self.even_when_paused = true;
        self
    }

    pub fn runsWhenPaused(&self) -> bool {
        self.even_when_paused
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

    pub fn withAttributes(
        &mut self,
        attributes: impl IntoIterator<Item = (String, Value)>,
    ) -> &mut Self {
        merge_attributes(&mut self.attributes, attributes);
        self
    }

    pub fn attributes(&self) -> &Map<String, Value> {
        &self.attributes
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

    pub(super) fn runs_in_environment(&self, environment: &str) -> bool {
        self.environments.is_empty() || self.environments.iter().any(|name| name == environment)
    }

    pub(super) fn filters_pass(&self, app: &Application) -> bool {
        self.filters.iter().all(|condition| condition(app))
    }

    pub fn getSummaryForDisplay(&self) -> String {
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

pub(super) fn parse_cron(expression: &str) -> Result<Cron, ConsoleError> {
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .sloppy_ranges(true)
        .build()
        .parse(expression)
        .map_err(super::error)
}

pub(super) fn daily_expression(time: &str) -> Result<String, ConsoleError> {
    let time = chrono::NaiveTime::parse_from_str(time, "%H:%M").map_err(super::error)?;
    Ok(format!("{} {} * * *", time.minute(), time.hour()))
}

pub(super) fn repeat_interval(seconds: u32) -> Result<Duration, ConsoleError> {
    if seconds == 0 || seconds >= 60 || !60_u32.is_multiple_of(seconds) {
        return Err(ConsoleError::new(
            "A repeat interval must divide one minute and be less than 60 seconds.",
        ));
    }
    Ok(Duration::from_secs(seconds.into()))
}

fn merge_attributes(
    target: &mut Map<String, Value>,
    source: impl IntoIterator<Item = (String, Value)>,
) {
    for (key, value) in source {
        match target.entry(key) {
            serde_json::map::Entry::Vacant(entry) => {
                entry.insert(value);
            }
            serde_json::map::Entry::Occupied(mut entry) => {
                let previous = std::mem::take(entry.get_mut());
                *entry.get_mut() = merge_attribute(previous, value);
            }
        }
    }
}

fn merge_attribute(left: Value, right: Value) -> Value {
    fn list(value: Value) -> Vec<Value> {
        match value {
            Value::Array(items) => items,
            item => vec![item],
        }
    }

    fn append(object: &mut Map<String, Value>, values: Vec<Value>) {
        let mut index = 0;
        for value in values {
            while object.contains_key(&index.to_string()) {
                index += 1;
            }
            object.insert(index.to_string(), value);
            index += 1;
        }
    }

    match (left, right) {
        (Value::Object(mut left), Value::Object(right)) => {
            merge_attributes(&mut left, right);
            Value::Object(left)
        }
        (Value::Object(mut left), right) => {
            append(&mut left, list(right));
            Value::Object(left)
        }
        (left, Value::Object(right)) => {
            let mut result = Map::new();
            append(&mut result, list(left));
            merge_attributes(&mut result, right);
            Value::Object(result)
        }
        (left, right) => Value::Array(list(left).into_iter().chain(list(right)).collect()),
    }
}
