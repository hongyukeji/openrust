#[path = "../config_app.rs"]
mod app;
#[path = "../../../config/mod.rs"]
mod config;
#[path = "../../../database/migrations/mod.rs"]
mod migrations;
#[path = "../../../bootstrap/packages.rs"]
mod packages;

use std::{backtrace::Backtrace, fmt, io, process::ExitCode, sync::Arc, time::Duration};

use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{
        debug::{ExceptionBehavior, SharedException},
        queue::JobHandler,
    },
    foundation::{Application, configuration::Routing, run_application},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    queue::{DispatchOptions, JobContext, JobOptions, Payload, QueueFuture, QueueResult},
    support::{
        ProviderResult, ServiceProvider,
        facades::{Bus, Log, Queue, Route},
    },
};

struct BatchProbe;

#[derive(Debug)]
struct ReportProbe {
    trace: Backtrace,
    source: io::Error,
    ignored: bool,
}

impl fmt::Display for ReportProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("private-explicit-report")
    }
}

impl std::error::Error for ReportProbe {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl ExceptionBehavior for ReportProbe {
    fn context(&self) -> LogContext {
        [("stacktrace".into(), self.trace.to_string().into())]
            .into_iter()
            .collect()
    }
}

impl JobHandler for BatchProbe {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async {
            match payload.data().as_str() {
                Some("release-once") if context.attempts() == 1 => {
                    context.release(Duration::ZERO);
                }
                Some("hard-timeout") => tokio::time::sleep(Duration::from_secs(10)).await,
                _ => {}
            }
            Ok(())
        })
    }
}

struct FixtureProvider;

impl ServiceProvider for FixtureProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            Queue::register("telemetry.batch.item", Arc::new(BatchProbe))?;
            Ok(())
        })
    }
}

struct Probe(CommandDefinition);

impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let failed = input.matches().get_flag("fail");
            if self.0.name() == "telemetry:overload" {
                use openrust::contracts::{
                    container::Container as _, foundation::Application as _,
                };
                let channel = Log::channel(Some("telemetry")).map_err(ConsoleError::from_source)?;
                for _ in 0..4096 {
                    channel
                        .error("overload fixture record", &LogContext::new())
                        .map_err(ConsoleError::from_source)?;
                }
                let control = application
                    .container()
                    .make::<openrust_new_relic::NewRelic>()
                    .map_err(ConsoleError::from_source)?;
                control.flush().await.map_err(ConsoleError::from_source)?;
                let stats = control.export_stats();
                output
                    .write_raw(&format!(
                        "drops {} {}\n",
                        stats.dropped_spans, stats.dropped_logs
                    ))
                    .map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            if self.0.name() == "telemetry:lifecycle" {
                let case = input
                    .matches()
                    .get_one::<String>("input")
                    .expect("scenario");
                let mut payload = Payload::new("telemetry.batch.item", case)
                    .map_err(ConsoleError::from_source)?;
                if case == "hard-timeout" {
                    payload = payload.with_options(JobOptions {
                        hard_timeout: true,
                        fail_on_timeout: true,
                        timeout: Some(Duration::from_secs(1)),
                        ..Default::default()
                    });
                }
                Queue::dispatch(
                    payload,
                    DispatchOptions {
                        connection: Some(std::env::var("NEW_RELIC_TEST_QUEUE").expect("queue")),
                        queue: Some("telemetry".into()),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            if self.0.name() == "telemetry:batch" {
                let make = || {
                    Payload::new("telemetry.batch.item", "private-job-input")
                        .map_err(ConsoleError::from_source)
                };
                let isolated = make()?.with_options(JobOptions {
                    hard_timeout: true,
                    timeout: Some(Duration::from_secs(10)),
                    ..Default::default()
                });
                Bus::batch([make()?, isolated])
                    .on_connection(
                        std::env::var("NEW_RELIC_TEST_QUEUE").unwrap_or_else(|_| "database".into()),
                    )
                    .on_queue("telemetry")
                    .dispatch()
                    .await
                    .map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            if self.0.name() == "telemetry:enqueue" {
                let mut arguments = vec!["private-job-input".into()];
                if failed {
                    arguments.push("--fail".into());
                }
                openrust::support::facades::Artisan::queue("telemetry:probe", &arguments)
                    .on_connection(
                        std::env::var("NEW_RELIC_TEST_QUEUE").unwrap_or_else(|_| "database".into()),
                    )
                    .on_queue("telemetry")
                    .dispatch()
                    .await
                    .map_err(ConsoleError::from_source)?;
                return Ok(0);
            }
            if failed
                && input
                    .matches()
                    .get_one::<String>("input")
                    .is_some_and(|value| value == "private-job-input")
            {
                return Err(ConsoleError::new("private-job-error"));
            }
            Ok(i32::from(failed))
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for name in [
        "telemetry:probe",
        "telemetry:enqueue",
        "telemetry:batch",
        "telemetry:lifecycle",
        "telemetry:overload",
    ] {
        let command = clap::Command::new(name).arg(clap::Arg::new("input")).arg(
            clap::Arg::new("fail")
                .long("fail")
                .action(clap::ArgAction::SetTrue),
        );
        artisan.add_command(Box::new(Probe(CommandDefinition::new(command))))?;
    }
    Ok(())
}

fn probe(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let status = match request.route("id") {
            Some("fail") => {
                return Err(HttpError::Internal {
                    source: Box::new(std::io::Error::other("private-exception-message")),
                });
            }
            Some("log") => {
                use openrust::contracts::container::Container as _;
                let services = openrust::support::facades::App::get_container()?;
                let new_relic = services.make::<openrust_new_relic::NewRelic>()?;
                assert!(new_relic.name_transaction("custom linked request"));
                assert!(new_relic.add_attribute("operation", "fixture"));
                let channel = openrust::support::facades::Log::channel(Some("telemetry")).map_err(
                    |source| HttpError::Internal {
                        source: Box::new(source),
                    },
                )?;
                channel
                    .error(
                        "linked fixture record",
                        &[
                            ("operation".into(), serde_json::json!("sample")),
                            (
                                "transaction_name".into(),
                                serde_json::json!("logged transaction"),
                            ),
                            ("secret".into(), serde_json::json!("private-log-context")),
                        ]
                        .into_iter()
                        .collect(),
                    )
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                StatusCode::CREATED
            }
            Some("buffer-retain" | "buffer-flush") => {
                let channel = Log::channel(Some("telemetry_stack")).map_err(|source| {
                    HttpError::Internal {
                        source: Box::new(source),
                    }
                })?;
                let pending = request.route("id") == Some("buffer-retain");
                let result = if pending {
                    channel.info(
                        "buffered fixture record",
                        &[(
                            "transaction_name".into(),
                            serde_json::json!("buffered transaction"),
                        )]
                        .into_iter()
                        .collect(),
                    )
                } else {
                    channel.error("flush fixture record", &LogContext::new())
                };
                result.map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
                if pending {
                    StatusCode::OK
                } else {
                    StatusCode::PARTIAL_CONTENT
                }
            }
            Some("report") => {
                let make = |ignored| {
                    SharedException::new(ReportProbe {
                        trace: Backtrace::force_capture(),
                        source: io::Error::other("private-explicit-source"),
                        ignored,
                    })
                };
                let report = async |exception| {
                    openrust::support::report(exception)
                        .await
                        .map_err(|source| HttpError::Internal {
                            source: Box::new(source),
                        })
                };
                let repeated = make(false);
                report(repeated.clone()).await?;
                report(repeated).await?;
                report(make(false)).await?;
                report(make(false)).await?;
                report(make(true)).await?;
                StatusCode::RESET_CONTENT
            }
            Some("flush") => {
                use openrust::contracts::container::Container as _;
                let services = openrust::support::facades::App::get_container()?;
                let new_relic = services.make::<openrust_new_relic::NewRelic>()?;
                new_relic
                    .flush()
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                assert!(new_relic.export_stats().attempts > 0);
                assert_eq!(new_relic.export_stats().rejected_items, 6);
                StatusCode::NO_CONTENT
            }
            Some("dependency") => {
                for variable in [
                    "NEW_RELIC_TEST_ALLOWED_UPSTREAM",
                    "NEW_RELIC_TEST_OTHER_UPSTREAM",
                    "NEW_RELIC_TEST_REDIRECT_UPSTREAM",
                ] {
                    let url = std::env::var(variable).expect("local test endpoint");
                    let connected =
                        openrust::support::facades::Http::get(&url)
                            .await
                            .map_err(|source| HttpError::Internal {
                                source: Box::new(source),
                            })?;
                    assert_eq!(connected.status(), StatusCode::NO_CONTENT);
                }
                let upstream = std::env::var("NEW_RELIC_TEST_BODY_UPSTREAM")
                    .expect("local body test endpoint");
                use openrust::http::client::{Error, Http};
                let response = Http::get(&format!("{upstream}/slow-body"))
                    .await
                    .expect("read HTTP response body");
                assert_eq!(response.body().expect("buffered body").as_ref(), b"fixture");
                let timed_out = Http::timeout(Duration::from_millis(40))
                    .expect("configure request timeout")
                    .get(&format!("{upstream}/slow-body"))
                    .await;
                assert!(matches!(timed_out, Err(Error::Timeout)));
                let interrupted = Http::get(&format!("{upstream}/interrupted-body")).await;
                assert!(matches!(interrupted, Err(Error::Connection)));
                let retried = Http::retry(2, Duration::ZERO)
                    .expect("configure request retry")
                    .get(&format!("{upstream}/retry"))
                    .await
                    .expect("retry after service failure");
                assert_eq!(retried.status(), StatusCode::NO_CONTENT);
                let slow_url = format!("{upstream}/slow-body");
                assert!(
                    tokio::time::timeout(Duration::from_millis(80), Http::get(&slow_url))
                        .await
                        .is_err()
                );
                let db = openrust::support::facades::Db::connection(None)?;
                db.select("SELECT 'private-sql-literal'", &[]).await?;
                assert!(
                    db.select("SELECT * FROM private_missing_table", &[])
                        .await
                        .is_err()
                );
                let mut rows = db.cursor("SELECT 1", &[]);
                while let Some(row) = std::future::poll_fn(|cx| rows.as_mut().poll_next(cx)).await {
                    row?;
                }
                drop(rows);
                let mut transaction = db.begin_transaction().await?;
                transaction.select("SELECT 1", &[]).await?;
                transaction.commit().await?;
                StatusCode::ACCEPTED
            }
            _ => StatusCode::NO_CONTENT,
        };
        Ok(Response::empty(status))
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1).is_none() && std::env::var_os("NEW_RELIC_TEST_HTTP").is_none() {
        return ExitCode::SUCCESS;
    }
    run_application(|| {
        let root = Application::infer_base_path(Some(env!("CARGO_MANIFEST_DIR")))?;
        Application::configure(root)
            .with_config(|path, env| {
                let mut config = config::load(path, env)?;
                let channels = &mut config.get_mut("logging").expect("logging config")["channels"];
                channels["telemetry"] = serde_json::json!({
                    "driver": "monolog",
                    "handler": "new_relic",
                    "formatter": "default",
                    "level": "error",
                        "with": {
                            "fields": ["context.operation", "context.transaction_name"],
                            "transaction_name": "channel default transaction",
                        },
                });
                channels["telemetry_buffer"] = serde_json::json!({
                    "driver": "monolog",
                    "handler": "new_relic",
                    "formatter": "default",
                    "level": "info",
                    "action_level": "error",
                    "with": {
                        "report_errors": false,
                        "fields": ["context.transaction_name"],
                    },
                });
                channels["telemetry_stack"] = serde_json::json!({
                    "driver": "stack",
                    "channels": ["telemetry_buffer", "telemetry_file"],
                });
                channels["telemetry_file"] = serde_json::json!({
                    "driver": "single",
                    "path": path.join("parallel-logs.log"),
                    "level": "info",
                });
                if env.get_utf8("NEW_RELIC_TEST_FAILOVER")? == Some("true") {
                    config.get_mut("queue").expect("queue config")["connections"]["failover"]
                        ["connections"] = serde_json::json!(["missing_fixture_connection", "deferred"]);
                }
                Ok(config)
            })
            .with_exceptions(|exceptions| {
                exceptions
                    .use_exception::<ReportProbe>()
                    .dont_report_when(|exception| {
                        exception
                            .downcast_ref::<ReportProbe>()
                            .is_some_and(|probe| probe.ignored)
                    });
            })
            .with_packages(packages::manifest())
            .with_providers(vec![Box::new(FixtureProvider)])
            .with_commands(commands)
            .with_migrations(migrations::entries())
            .with_routing(Routing::default().api(|| Route::get("/probe/{id}", probe)))
            .create()
            .map_err(Into::into)
    })
    .await
}
