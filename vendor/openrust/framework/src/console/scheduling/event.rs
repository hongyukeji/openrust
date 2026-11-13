use std::{ffi::OsString, fmt::Write, path::PathBuf, sync::Arc, time::Duration};

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

pub(super) type Callback = dyn for<'a> Fn(&'a Application, &'a mut Output) -> ConsoleFuture<'a, Result<(), ConsoleError>>
    + Send
    + Sync;
type OutputHook = dyn for<'a> Fn(
        &'a Application,
        &'a mut Output,
        &'a [u8],
    ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
    + Send
    + Sync;

enum AfterHook {
    Email {
        recipients: Vec<crate::mail::mailables::Address>,
        only_if_output: bool,
    },
    Plain(Arc<Callback>),
    WithOutput {
        callback: Arc<OutputHook>,
        only_if_output: bool,
    },
}

type Condition = dyn Fn(&Application) -> bool + Send + Sync;

#[derive(Clone, Copy)]
pub(super) enum HookKind {
    Before,
    After,
    Success,
    Failure,
}

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
    before_hooks: Vec<Arc<Callback>>,
    after_hooks: Vec<(HookKind, AfterHook)>,
    pub(super) destination: Option<super::task_output::Destination>,
    pub(super) background: bool,
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
            before_hooks: Vec::new(),
            after_hooks: Vec::new(),
            destination: None,
            background: false,
        }
    }

    pub fn before(
        &mut self,
        hook: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.add_hook(HookKind::Before, Arc::new(hook));
        self
    }

    pub fn after(
        &mut self,
        hook: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.add_hook(HookKind::After, Arc::new(hook));
        self
    }

    pub fn then(
        &mut self,
        hook: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.after(hook)
    }

    pub fn onSuccess(
        &mut self,
        hook: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.add_hook(HookKind::Success, Arc::new(hook));
        self
    }

    pub fn onFailure(
        &mut self,
        hook: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.add_hook(HookKind::Failure, Arc::new(hook));
        self
    }

    pub(super) fn add_hook(&mut self, kind: HookKind, hook: Arc<Callback>) {
        match kind {
            HookKind::Before => self.before_hooks.push(hook),
            _ => self.after_hooks.push((kind, AfterHook::Plain(hook))),
        }
    }

    pub(super) async fn call_before_hooks(
        &self,
        app: &Application,
        output: &mut Output,
    ) -> Result<(), ConsoleError> {
        for hook in &self.before_hooks {
            hook(app, output).await?;
        }
        Ok(())
    }

    pub(super) async fn call_after_hooks(
        &self,
        app: &Application,
        output: &mut Output,
        succeeded: bool,
        captured: &[u8],
    ) -> Result<(), ConsoleError> {
        for (kind, hook) in &self.after_hooks {
            let applies = match kind {
                HookKind::Before => unreachable!("before hooks are stored separately"),
                HookKind::After => true,
                HookKind::Success => succeeded,
                HookKind::Failure => !succeeded,
            };
            if applies {
                match hook {
                    AfterHook::Plain(hook) => hook(app, output).await?,
                    AfterHook::WithOutput {
                        callback,
                        only_if_output,
                    } if !only_if_output || !captured.is_empty() => {
                        callback(app, output, captured).await?;
                    }
                    AfterHook::Email {
                        recipients,
                        only_if_output,
                    } if !only_if_output || !captured.is_empty() => {
                        let subject = self.description.clone().unwrap_or_else(|| {
                            format!("Scheduled Job Output For [{}]", self.getSummaryForDisplay())
                        });
                        super::task_output::email(recipients, subject, captured).await?;
                    }
                    AfterHook::Email { .. } | AfterHook::WithOutput { .. } => {}
                }
            }
        }
        Ok(())
    }

    pub fn sendOutputTo(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.destination = Some(super::task_output::Destination {
            path: path.into(),
            append: false,
        });
        self
    }

    pub fn appendOutputTo(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.sendOutputTo(path);
        self.destination
            .as_mut()
            .expect("destination just set")
            .append = true;
        self
    }

    pub fn runInBackground(&mut self) -> &mut Self {
        self.background = true;
        self
    }

    pub(super) fn captures_output(&self) -> bool {
        self.after_hooks
            .iter()
            .any(|(_, hook)| matches!(hook, AfterHook::WithOutput { .. } | AfterHook::Email { .. }))
    }

    fn output_hook(
        &mut self,
        kind: HookKind,
        only_if_output: bool,
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
            &'a [u8],
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.after_hooks.push((
            kind,
            AfterHook::WithOutput {
                callback: Arc::new(callback),
                only_if_output,
            },
        ));
        self
    }

    pub fn thenWithOutput(
        &mut self,
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
            &'a [u8],
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
        only_if_output: bool,
    ) -> &mut Self {
        self.output_hook(HookKind::After, only_if_output, callback)
    }

    pub fn onSuccessWithOutput(
        &mut self,
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
            &'a [u8],
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
        only_if_output: bool,
    ) -> &mut Self {
        self.output_hook(HookKind::Success, only_if_output, callback)
    }

    pub fn onFailureWithOutput(
        &mut self,
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Output,
            &'a [u8],
        ) -> ConsoleFuture<'a, Result<(), ConsoleError>>
        + Send
        + Sync
        + 'static,
        only_if_output: bool,
    ) -> &mut Self {
        self.output_hook(HookKind::Failure, only_if_output, callback)
    }

    pub fn emailOutputTo(
        &mut self,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
    ) -> &mut Self {
        self.email_output(HookKind::After, recipients, true)
    }

    pub fn emailWrittenOutputTo(
        &mut self,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
    ) -> &mut Self {
        self.emailOutputTo(recipients)
    }

    pub fn emailOutputOnFailure(
        &mut self,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
    ) -> &mut Self {
        self.email_output(HookKind::Failure, recipients, false)
    }

    fn email_output(
        &mut self,
        kind: HookKind,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
        only_if_output: bool,
    ) -> &mut Self {
        let recipients = recipients.into_iter().map(Into::into).collect();
        self.after_hooks.push((
            kind,
            AfterHook::Email {
                recipients,
                only_if_output,
            },
        ));
        self
    }

    pub fn pingBefore(&mut self, url: impl Into<String>) -> &mut Self {
        self.pingBeforeIf(true, url)
    }

    pub fn pingBeforeIf(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::Before, url.into());
        }
        self
    }

    pub fn thenPing(&mut self, url: impl Into<String>) -> &mut Self {
        self.thenPingIf(true, url)
    }

    pub fn thenPingIf(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::After, url.into());
        }
        self
    }

    pub fn pingOnSuccess(&mut self, url: impl Into<String>) -> &mut Self {
        self.pingOnSuccessIf(true, url)
    }

    pub fn pingOnSuccessIf(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::Success, url.into());
        }
        self
    }

    pub fn pingOnFailure(&mut self, url: impl Into<String>) -> &mut Self {
        self.pingOnFailureIf(true, url)
    }

    pub fn pingOnFailureIf(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::Failure, url.into());
        }
        self
    }

    fn add_ping(&mut self, kind: HookKind, url: String) {
        let url: Arc<str> = Arc::from(url);
        self.add_hook(
            kind,
            Arc::new(move |app, _| {
                let url = url.clone();
                Box::pin(async move {
                    let result = async {
                        crate::support::facades::Http::pendingRequest()?
                            .timeout(Duration::from_secs(10))
                            .get(&url)
                            .await?
                            .throw()
                    }
                    .await;
                    if let Err(error) = result {
                        let _ = app.report(&error).await;
                    }
                    Ok(())
                })
            }),
        );
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

    pub(super) fn command_name(&self) -> String {
        match &self.action {
            Action::Command(args) | Action::Exec(args) => args
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            _ => self.getSummaryForDisplay(),
        }
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
