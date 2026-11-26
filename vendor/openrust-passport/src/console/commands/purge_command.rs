use crate::Passport;
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::Date,
};

pub struct PurgeCommand {
    definition: CommandDefinition,
}
impl Default for PurgeCommand {
    fn default() -> Self {
        Self {
            definition: CommandDefinition::new(
                Syntax::new("passport:purge")
                    .about("Purge revoked and / or expired tokens and authentication codes")
                    .arg(
                        Arg::new("revoked")
                            .long("revoked")
                            .action(ArgAction::SetTrue)
                            .help("Only purge revoked tokens and authentication codes"),
                    )
                    .arg(
                        Arg::new("expired")
                            .long("expired")
                            .action(ArgAction::SetTrue)
                            .help("Only purge expired tokens and authentication codes"),
                    )
                    .arg(
                        Arg::new("hours")
                            .long("hours")
                            .default_value("168")
                            .allow_hyphen_values(true)
                            .help("The number of hours to retain expired tokens"),
                    ),
            ),
        }
    }
}
impl Command for PurgeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let revoked =
                input.matches().get_flag("revoked") || !input.matches().get_flag("expired");
            let expired =
                input.matches().get_flag("expired") || !input.matches().get_flag("revoked");
            let hours = if expired {
                input
                    .matches()
                    .get_one::<String>("hours")
                    .expect("default hours")
                    .parse::<f64>()
                    .map_err(error)?
            } else {
                0.
            };
            if !hours.is_finite() || (hours * 3600. * 1_000_000.).abs() >= i64::MAX as f64 {
                return Err(ConsoleError::new("Invalid purge hours"));
            }
            let current = Date::now(None).map_err(error)?;
            let now = current.as_date_time();
            let cutoff = now
                .checked_sub(jiff::SignedDuration::from_micros(
                    (hours * 3600. * 1_000_000.) as i64,
                ))
                .map_err(|_| ConsoleError::new("Invalid purge cutoff"))?;
            let passport = app.make::<Passport>().await.map_err(error)?;
            passport
                .token_models
                .purge(
                    &passport.grant_models,
                    revoked,
                    expired.then(|| cutoff.strftime("%Y-%m-%d %H:%M:%S%.6f").to_string()),
                    now.strftime("%Y-%m-%d %H:%M:%S%.6f").to_string(),
                    passport
                        .device_code_grant_enabled
                        .load(std::sync::atomic::Ordering::Relaxed),
                )
                .await
                .map_err(error)?;
            let mut items = Vec::new();
            if revoked {
                items.push("revoked items".to_owned());
            }
            if expired {
                items.push(format!(
                    "items expired for more than {}",
                    human_interval(cutoff, now.clone())?
                ));
            }
            output.info(&format!("Purged {}.", items.join(" and ")));
            Ok(0)
        })
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
fn human_interval(start: jiff::Zoned, end: jiff::Zoned) -> Result<String, ConsoleError> {
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let mut months = i64::from(end.year() - start.year()) * 12 + i64::from(end.month())
        - i64::from(start.month());
    let candidate = Date::from_utc(start.timestamp(), start.time_zone().clone())
        .plus(
            openrust::support::DateInterval {
                months,
                ..Default::default()
            },
            openrust::support::MonthOverflow::Clamp,
        )
        .map_err(error)?;
    if candidate.as_date_time() > end {
        months -= 1;
    }
    let plural = |n, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    if months >= 12 {
        return Ok(plural(months / 12, "year"));
    }
    if months >= 1 {
        return Ok(plural(months, "month"));
    }
    let seconds = end.duration_since(&start).as_secs().unsigned_abs();
    for (unit, size) in [
        ("week", 604800),
        ("day", 86400),
        ("hour", 3600),
        ("minute", 60),
        ("second", 1),
    ] {
        if seconds >= size {
            return Ok(plural((seconds / size) as i64, unit));
        }
    }
    Ok("1 second".into())
}
