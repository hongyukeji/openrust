use clap::{Arg, ArgAction, Command as Syntax};
use openrust::contracts::foundation::Application as _;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{
        cache::{LockProvider as _, Repository as _},
        events::Dispatcher as _,
    },
    foundation::Application,
    support::facades::Artisan,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct PulseCommand(CommandDefinition, Kind);
#[derive(Clone, Copy)]
enum Kind {
    Clear,
    Check,
    Work,
    Restart,
}
pub(crate) fn register() -> Result<(), ConsoleError> {
    for (syntax, kind) in [
        (
            Syntax::new("pulse:clear")
                .alias("pulse:purge")
                .about("Delete all Pulse data from storage")
                .arg(Arg::new("type").long("type").action(ArgAction::Append))
                .arg(Arg::new("force").long("force").action(ArgAction::SetTrue)),
            Kind::Clear,
        ),
        (
            Syntax::new("pulse:check")
                .about("Take a snapshot of the current server's pulse")
                .arg(Arg::new("once").long("once").action(ArgAction::SetTrue)),
            Kind::Check,
        ),
        (
            Syntax::new("pulse:work")
                .about("Process incoming Pulse data from the ingest stream")
                .arg(
                    Arg::new("stop-when-empty")
                        .long("stop-when-empty")
                        .action(ArgAction::SetTrue),
                ),
            Kind::Work,
        ),
        (
            Syntax::new("pulse:restart").about("Restart any running work and check commands"),
            Kind::Restart,
        ),
    ] {
        Artisan::add(Box::new(PulseCommand(CommandDefinition::new(syntax), kind)))?;
    }
    Ok(())
}
fn error(e: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(e.to_string())
}
pub(crate) async fn cache(
    app: &Application,
) -> Result<Arc<openrust::cache::Repository>, ConsoleError> {
    let name = app
        .config()
        .map_err(error)?
        .get("pulse.cache")
        .and_then(|v| v.as_str().map(str::to_owned));
    app.make::<openrust::cache::CacheManager>()
        .await
        .map_err(error)?
        .store(name.as_deref())
        .map_err(error)
}
impl Command for PulseCommand {
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
            let pulse = app.make::<crate::Pulse>().await.map_err(error)?;
            match self.1 {
                Kind::Clear => {
                    let production = app
                        .config()
                        .map_err(error)?
                        .get("app.env")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .as_deref()
                        == Some("production");
                    if !openrust::console::confirmToProceed(production, input.matches())? {
                        return Ok(1);
                    }
                    let types = input
                        .matches()
                        .get_many::<String>("type")
                        .map(|v| v.cloned().collect::<Vec<_>>());
                    pulse.purge(types.as_deref()).await.map_err(error)?;
                    output.info(
                        &types
                            .map(|t| format!("Purging Pulse data for [{}]", t.join(", ")))
                            .unwrap_or("Purging all Pulse data".into()),
                    );
                }
                Kind::Restart => {
                    cache(app)
                        .await?
                        .forever(
                            "openrust:pulse:restart",
                            &openrust::cache::Value::from(jiff::Timestamp::now().as_second()),
                        )
                        .await
                        .map_err(error)?;
                    output.info("Broadcasting pulse restart signal.");
                }
                kind => {
                    let cache = cache(app).await?;
                    let restart = cache.get("openrust:pulse:restart").await.map_err(error)?;
                    let mut trimmed = jiff::Timestamp::now().as_second() / 60 * 60;
                    let dispatcher = app
                        .make::<openrust::events::Dispatcher>()
                        .await
                        .map_err(error)?;
                    let vapor = matches!(kind, Kind::Check)
                        && app
                            .make::<openrust::config::Environment>()
                            .await
                            .map_err(error)?
                            .get_utf8("VAPOR_SSM_PATH")
                            .map_err(error)?
                            .is_some_and(|v| !v.is_empty() && v != "0");
                    let instance = if vapor {
                        "vapor".into()
                    } else {
                        format!("{:032x}", fastrand::u128(..))
                    };
                    let mut lock = if matches!(kind, Kind::Check)
                        && cache.get_store().lock_provider().is_some()
                    {
                        Some(
                            cache
                                .lock("openrust:pulse:check", Some(Duration::from_secs(1)), None)
                                .map_err(error)?,
                        )
                    } else {
                        None
                    };
                    loop {
                        let started = Instant::now();
                        let now = jiff::Timestamp::now().as_second();
                        if cache.get("openrust:pulse:restart").await.map_err(error)? != restart {
                            return Ok(0);
                        }
                        if matches!(kind, Kind::Check) {
                            if let Some(lock) = &mut lock
                                && lock.get().await.map_err(error)?
                            {
                                dispatcher
                                    .dispatch(
                                        Arc::new(crate::events::IsolatedBeat { time: now }),
                                        &mut openrust::events::EventContext::new(),
                                    )
                                    .await
                                    .map_err(error)?;
                            }
                            dispatcher
                                .dispatch(
                                    Arc::new(crate::events::SharedBeat {
                                        time: now,
                                        instance: instance.clone(),
                                    }),
                                    &mut openrust::events::EventContext::new(),
                                )
                                .await
                                .map_err(error)?;
                            pulse.ingest().await;
                            collectTelescope(app).await?;
                            if vapor || input.matches().get_flag("once") {
                                break;
                            }
                        } else {
                            pulse.digest().await.map_err(error)?;
                            if now - 600 > trimmed {
                                pulse.trim().await.map_err(error)?;
                                trimmed = now;
                            }
                            collectTelescope(app).await?;
                            if input.matches().get_flag("stop-when-empty") {
                                break;
                            }
                        }
                        tokio::time::sleep(
                            Duration::from_secs(1).saturating_sub(started.elapsed()),
                        )
                        .await;
                    }
                }
            }
            Ok(0)
        })
    }
}

async fn collectTelescope(_app: &Application) -> Result<(), ConsoleError> {
    #[cfg(feature = "telescope")]
    {
        use openrust::contracts::container::Container as _;
        if _app
            .container()
            .bound::<dyn openrust_telescope::contracts::EntriesRepository>()
        {
            let telescope = _app
                .make::<openrust_telescope::Telescope>()
                .await
                .map_err(error)?;
            let pending = telescope.store().await.map_err(error)?;
            if !pending.is_empty() {
                openrust_telescope::jobs::ProcessPendingUpdates::new(pending, 0)
                    .dispatch()
                    .await
                    .map_err(error)?;
            }
        }
    }
    Ok(())
}
