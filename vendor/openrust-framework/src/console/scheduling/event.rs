use std::{ffi::OsString, fmt::Write, path::PathBuf, sync::Arc, time::Duration};

use croner::{
    Cron,
    parser::{CronParser, Seconds, Year},
};
use jiff::{Timestamp, tz::TimeZone};
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
    Callback(Box<Callback>, &'static std::panic::Location<'static>),
    Command(Vec<OsString>),
    Exec(Vec<OsString>),
    Job(Box<Payload>, DispatchOptions),
}

/// A scheduled task with a frequency, time zone, and execution constraints.
pub struct Event {
    pub(super) action: Action,
    pub(super) expression: Cron,
    pub(super) cron_text: String,
    pub(super) zone: Option<TimeZone>,
    pub(super) interval: Option<Duration>,
    pub(super) overlap: Option<Duration>,
    pub(super) one_server: bool,
    pub(super) description: Option<String>,
    user: Option<String>,
    even_in_maintenance_mode: bool,
    even_when_paused: bool,
    pub(super) environments: Vec<String>,
    filters: Vec<Box<Condition>>,
    attributes: Map<String, Value>,
    before_hooks: Vec<Arc<Callback>>,
    after_hooks: Vec<(HookKind, AfterHook)>,
    pub(super) destination: Option<super::task_output::Destination>,
    pub(super) background: bool,
}

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
            user: None,
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

    pub fn on_success(
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

    pub fn on_failure(
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
        recorded_timezone: Option<&TimeZone>,
        scheduler_command: &'static str,
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
                            format!(
                                "Scheduled Job Output For [{}]",
                                self.get_summary_for_display()
                            )
                        });
                        super::task_output::email(recipients, subject, captured).await?;
                    }
                    AfterHook::Email { .. } | AfterHook::WithOutput { .. } => {}
                }
            }
        }
        use crate::{
            contracts::{events::Dispatcher as _, foundation::Application as _},
            events::EventContext,
            support::facades::Event as Events,
        };
        let description = self.description.clone().or_else(|| match &self.action {
            Action::Job(payload, _) => Some(payload.display_name().to_owned()),
            _ => None,
        });
        let completed = Arc::new(crate::console::events::ScheduledTaskCompleted {
            command: match self.action {
                Action::Callback(..) | Action::Job(..) => "Closure".into(),
                _ => self.command_name(),
            },
            description,
            expression: self.cron_text.clone(),
            timezone: recorded_timezone.map(|zone| {
                zone.iana_name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| zone.to_offset(Timestamp::now()).to_string())
            }),
            user: self.user.clone(),
            output: self
                .destination
                .as_ref()
                .map(|destination| app.base_path().join(&destination.path)),
            should_append_output: self
                .destination
                .as_ref()
                .is_some_and(|destination| destination.append),
        });
        let arguments: [&(dyn std::any::Any + Send + Sync); 1] = [&scheduler_command];
        let mut context = EventContext::with_arguments(completed.as_ref(), &arguments);
        Events::dispatcher()
            .map_err(super::error)?
            .dispatch(completed.clone(), &mut context)
            .await
            .map_err(super::error)?;
        Ok(())
    }

    /// 为 Unix 命令配置运行用户。回调和作业保留此属性用于报告。
    pub fn user(&mut self, user: impl Into<String>) -> &mut Self {
        self.user = Some(user.into());
        self
    }

    pub(super) fn execution_user(&self) -> Option<&str> {
        if cfg!(unix) && matches!(self.action, Action::Command(_) | Action::Exec(_)) {
            self.user
                .as_deref()
                .filter(|user| !matches!(*user, "" | "0"))
        } else {
            None
        }
    }

    pub fn send_output_to(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.destination = Some(super::task_output::Destination {
            path: path.into(),
            append: false,
        });
        self
    }

    pub fn append_output_to(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.send_output_to(path);
        self.destination
            .as_mut()
            .expect("destination just set")
            .append = true;
        self
    }

    pub fn run_in_background(&mut self) -> &mut Self {
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

    pub fn then_with_output(
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

    pub fn on_success_with_output(
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

    pub fn on_failure_with_output(
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

    pub fn email_output_to(
        &mut self,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
    ) -> &mut Self {
        self.email_output(HookKind::After, recipients, true)
    }

    pub fn email_written_output_to(
        &mut self,
        recipients: impl IntoIterator<Item = impl Into<crate::mail::mailables::Address>>,
    ) -> &mut Self {
        self.email_output_to(recipients)
    }

    pub fn email_output_on_failure(
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

    pub fn ping_before(&mut self, url: impl Into<String>) -> &mut Self {
        self.ping_before_if(true, url)
    }

    pub fn ping_before_if(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::Before, url.into());
        }
        self
    }

    pub fn then_ping(&mut self, url: impl Into<String>) -> &mut Self {
        self.then_ping_if(true, url)
    }

    pub fn then_ping_if(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::After, url.into());
        }
        self
    }

    pub fn ping_on_success(&mut self, url: impl Into<String>) -> &mut Self {
        self.ping_on_success_if(true, url)
    }

    pub fn ping_on_success_if(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
        if enabled {
            self.add_ping(HookKind::Success, url.into());
        }
        self
    }

    pub fn ping_on_failure(&mut self, url: impl Into<String>) -> &mut Self {
        self.ping_on_failure_if(true, url)
    }

    pub fn ping_on_failure_if(&mut self, enabled: bool, url: impl Into<String>) -> &mut Self {
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
                        crate::support::facades::Http::pending_request()?
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

    pub fn every_minute(&mut self) -> &mut Self {
        self.cron("* * * * *").expect("valid minute frequency")
    }

    pub fn daily(&mut self) -> &mut Self {
        self.cron("0 0 * * *").expect("valid daily frequency")
    }

    pub fn twice_daily(&mut self) -> &mut Self {
        self.cron("0 1,13 * * *")
            .expect("valid twice-daily frequency")
    }

    pub fn daily_at(&mut self, time: &str) -> Result<&mut Self, ConsoleError> {
        self.cron(&daily_expression(time)?)
    }

    pub fn every_seconds(&mut self, seconds: u32) -> Result<&mut Self, ConsoleError> {
        let interval = repeat_interval(seconds)?;
        self.every_minute();
        self.interval = Some(interval);
        Ok(self)
    }

    pub(super) fn set_frequency(&mut self, cron: Cron, text: &str, interval: Option<Duration>) {
        self.expression = cron;
        self.cron_text = text.into();
        self.interval = interval;
    }

    pub fn every_second(&mut self) -> &mut Self {
        self.repeat(1)
    }
    pub fn every_two_seconds(&mut self) -> &mut Self {
        self.repeat(2)
    }
    pub fn every_five_seconds(&mut self) -> &mut Self {
        self.repeat(5)
    }
    pub fn every_ten_seconds(&mut self) -> &mut Self {
        self.repeat(10)
    }
    pub fn every_fifteen_seconds(&mut self) -> &mut Self {
        self.repeat(15)
    }
    pub fn every_twenty_seconds(&mut self) -> &mut Self {
        self.repeat(20)
    }
    pub fn every_thirty_seconds(&mut self) -> &mut Self {
        self.repeat(30)
    }

    fn repeat(&mut self, seconds: u32) -> &mut Self {
        self.every_seconds(seconds).expect("valid repeat frequency")
    }

    pub fn timezone(&mut self, zone: TimeZone) -> &mut Self {
        self.zone = Some(zone);
        self
    }

    pub fn environments(&mut self, names: &[&str]) -> &mut Self {
        self.environments = names.iter().map(|name| (*name).into()).collect();
        self
    }

    /// Allows this event to run while the application is in maintenance mode.
    pub fn even_in_maintenance_mode(&mut self) -> &mut Self {
        self.even_in_maintenance_mode = true;
        self
    }

    pub fn runs_in_maintenance_mode(&self) -> bool {
        self.even_in_maintenance_mode
    }

    /// Allows this event to run while the scheduler is paused.
    pub fn even_when_paused(&mut self) -> &mut Self {
        self.even_when_paused = true;
        self
    }

    pub fn runs_when_paused(&self) -> bool {
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

    pub fn with_attributes(
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

    pub fn without_overlapping(&mut self) -> &mut Self {
        self.overlap = Some(Duration::from_secs(1440 * 60));
        self
    }

    pub fn without_overlapping_for(&mut self, minutes: u32) -> Result<&mut Self, ConsoleError> {
        if minutes == 0 {
            return Err(ConsoleError::new(
                "An overlap lock requires a positive expiry in minutes.",
            ));
        }
        self.overlap = Some(Duration::from_secs(u64::from(minutes) * 60));
        Ok(self)
    }

    pub fn on_one_server(&mut self) -> &mut Self {
        self.one_server = true;
        self
    }

    pub(super) fn due(&self, now: Timestamp, zone: TimeZone) -> Result<bool, ConsoleError> {
        let minute = now
            .to_zoned(TimeZone::UTC)
            .with()
            .second(0)
            .build()
            .map_err(super::error)?;
        self.expression
            .is_time_matching(&minute.with_time_zone(self.zone.clone().unwrap_or(zone)))
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
            _ => self.get_summary_for_display(),
        }
    }

    pub fn get_summary_for_display(&self) -> String {
        self.description
            .clone()
            .unwrap_or_else(|| match &self.action {
                Action::Callback(..) => "Closure".into(),
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
            Action::Callback(..) => field(b"callback"),
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
    let time = jiff::civil::Time::strptime("%H:%M", time).map_err(super::error)?;
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
