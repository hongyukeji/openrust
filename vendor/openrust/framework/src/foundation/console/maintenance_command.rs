use std::fmt::{Display, Write as _};

use chrono::{DateTime, NaiveDateTime, NaiveTime, TimeDelta, TimeZone as _, Utc};
use chrono_tz::Tz;
use clap::{Arg, ArgAction};
use serde_json::{Value, json};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{
        Application,
        events::{MaintenanceModeDisabled, MaintenanceModeEnabled},
        maintenance::MaintenancePayload,
    },
    support::facades::Event,
    view::{Factory, ViewData},
};

struct DownCommand(CommandDefinition);

impl Command for DownCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mode = app.maintenanceMode().map_err(down_error)?;
            let already_down = mode.active().await.map_err(down_error)?;
            let matches = input.matches();
            let value = |name| matches.get_one::<String>(name).map(String::as_str);
            let redirect = value("redirect").map(local_path).transpose()?;
            let secret = match value("secret") {
                Some(secret) => {
                    if secret.is_empty() || secret != secret.trim_matches('/') {
                        return Err(down_error(
                            "secret must be a nonempty path without surrounding slashes",
                        ));
                    }
                    local_path(secret)?;
                    Some(secret.to_owned())
                }
                None if matches.get_flag("with-secret") => Some(generate_secret()?),
                None => None,
            };
            let config = app.configuration_repository().map_err(down_error)?;
            let timezone = config.get("app.timezone");
            let timezone = timezone
                .as_ref()
                .and_then(Value::as_str)
                .and_then(|timezone| timezone.parse::<Tz>().ok())
                .unwrap_or(chrono_tz::UTC);
            let retry = value("retry").and_then(|value| retry_value(value, timezone));
            let refresh = matches.get_one::<u64>("refresh");
            let status = *matches.get_one::<u16>("status").expect("default status");
            let template = value("render")
                .map(|name| render(app, name, value("retry")))
                .transpose()?;
            let url = config.get("app.url");
            let url = url.as_ref().and_then(Value::as_str).unwrap_or_default();
            let bypass_url = secret
                .as_ref()
                .map(|secret| format!("{}/{secret}", url.trim_end_matches('/')));
            let payload = [
                (
                    "except",
                    json!(app.configured_middleware().maintenance_exceptions),
                ),
                ("redirect", json!(redirect)),
                ("retry", json!(retry)),
                ("refresh", json!(refresh)),
                ("secret", json!(secret)),
                ("status", json!(status)),
                ("template", json!(template)),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<MaintenancePayload>();
            mode.activate(&payload).await.map_err(down_error)?;
            Event::dispatch(MaintenanceModeEnabled)
                .await
                .map_err(down_error)?;
            output.info(if already_down {
                "Maintenance mode options updated."
            } else {
                "Application is now in maintenance mode."
            });
            if let Some(url) = bypass_url {
                output.info(&format!("You may bypass maintenance mode via [{url}]."));
            }
            Ok(0)
        })
    }
}

struct UpCommand(CommandDefinition);

impl Command for UpCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let mode = app.maintenanceMode().map_err(up_error)?;
            if !mode.active().await.map_err(up_error)? {
                output.info("Application is already up.");
                return Ok(0);
            }
            mode.deactivate().await.map_err(up_error)?;
            Event::dispatch(MaintenanceModeDisabled)
                .await
                .map_err(up_error)?;
            output.info("Application is now live.");
            Ok(0)
        })
    }
}

fn local_path(value: &str) -> Result<String, ConsoleError> {
    let path = format!("/{}", value.trim_matches('/'));
    if path.contains(['\\', '?', '#']) || path.parse::<::http::uri::PathAndQuery>().is_err() {
        return Err(down_error("expected a local URL path"));
    }
    Ok(path)
}

fn retry_value(value: &str, timezone: Tz) -> Option<Value> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>()
        && seconds > 0
    {
        return Some(seconds.into());
    }
    let date = retry_datetime(value, timezone, std::time::SystemTime::now().into())?;
    Some(date.format("%a, %d %b %Y %H:%M:%S GMT").to_string().into())
}

fn retry_datetime(value: &str, timezone: Tz, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Ok(date) =
        DateTime::parse_from_rfc3339(value).or_else(|_| DateTime::parse_from_rfc2822(value))
    {
        return Some(date.to_utc());
    }
    if let Some(timestamp) = value.strip_prefix('@') {
        return DateTime::from_timestamp(timestamp.parse().ok()?, 0);
    }
    if let Ok(date) = NaiveDateTime::parse_from_str(value, "%a, %d %b %Y %H:%M:%S GMT") {
        return Some(date.and_utc());
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return timezone
                .from_local_datetime(&date)
                .earliest()
                .map(|date| date.to_utc());
        }
    }

    let (relative, time) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
    let time = time.trim();
    let day_offset = match relative {
        "yesterday" => -1,
        "today" => 0,
        "tomorrow" => 1,
        _ => {
            if !relative.starts_with(['+', '-']) {
                return None;
            }
            let count: i64 = relative.parse().ok()?;
            let duration = match time.trim_end_matches('s') {
                "second" => TimeDelta::try_seconds(count),
                "minute" => TimeDelta::try_minutes(count),
                "hour" => TimeDelta::try_hours(count),
                "day" => TimeDelta::try_days(count),
                "week" => TimeDelta::try_weeks(count),
                _ => None,
            }?;
            return now.checked_add_signed(duration);
        }
    };
    let day = now
        .with_timezone(&timezone)
        .date_naive()
        .checked_add_signed(TimeDelta::days(day_offset))?;
    let time = if time.is_empty() {
        NaiveTime::MIN
    } else {
        NaiveTime::parse_from_str(time, "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(time, "%H:%M:%S"))
            .ok()?
    };
    timezone
        .from_local_datetime(&day.and_time(time))
        .earliest()
        .map(|date| date.to_utc())
}

fn render(app: &Application, name: &str, retry: Option<&str>) -> Result<String, ConsoleError> {
    let factory = app.container().make::<Factory>().map_err(down_error)?;
    let mut data = ViewData::new();
    data.insert("retryAfter".into(), json!(retry));
    if name == "errors::503" {
        if factory.exists("errors.503") {
            return factory.render("errors.503", &data).map_err(down_error);
        }
        return Ok(crate::foundation::exceptions::maintenance_page());
    }
    factory.render(name, &data).map_err(down_error)
}

fn generate_secret() -> Result<String, ConsoleError> {
    let mut bytes = [0_u8; 16];
    aws_lc_rs::rand::fill(&mut bytes).map_err(down_error)?;
    let mut secret = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(secret, "{byte:02x}").expect("writing to a String succeeds");
    }
    Ok(secret)
}

fn down_error(error: impl Display) -> ConsoleError {
    ConsoleError::new(format!("Failed to enter maintenance mode: {error}."))
}

fn up_error(error: impl Display) -> ConsoleError {
    ConsoleError::new(format!("Failed to disable maintenance mode: {error}."))
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let option = |name: &'static str| Arg::new(name).long(name);
    artisan.add_command(Box::new(DownCommand(CommandDefinition::new(
        clap::Command::new("down")
            .about("Put the application into maintenance / demo mode")
            .arg(option("redirect").help("The path to redirect users to"))
            .arg(option("render").help("The view to prerender"))
            .arg(option("retry").help("Retry delay in seconds or the retry date/time"))
            .arg(
                option("refresh")
                    .value_parser(clap::value_parser!(u64))
                    .help("Refresh delay in seconds"),
            )
            .arg(option("secret").help("The path that grants bypass access"))
            .arg(
                option("with-secret")
                    .action(ArgAction::SetTrue)
                    .help("Generate a random bypass secret"),
            )
            .arg(
                option("status")
                    .default_value("503")
                    .value_parser(clap::value_parser!(u16).range(100..=599))
                    .help("The HTTP response status"),
            ),
    ))))?;
    artisan.add_command(Box::new(UpCommand(CommandDefinition::new(
        clap::Command::new("up").about("Bring the application out of maintenance mode"),
    ))))
}
