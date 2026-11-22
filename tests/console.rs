use std::{
    ffi::OsString,
    sync::atomic::{AtomicUsize, Ordering},
};

use clap::Arg;
use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{console::Kernel as _, foundation::Application as _, http::Kernel as _},
    foundation::{Application, BootstrapError, configuration::Routing, console::Kernel, http},
    http::{Body, Request, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan as ArtisanFacade, Route},
    },
    view::view,
};

#[path = "../routes/console.rs"]
mod application_console;

struct Greet(CommandDefinition);

impl Command for Greet {
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
            assert!(app.is_booted());
            let name = input.matches().get_one::<String>("name").unwrap();
            output.stdout.push_str(&format!("Hello {name}\n"));
            Ok(7)
        })
    }
}

struct NestedArtisan(CommandDefinition);

impl Command for NestedArtisan {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let arguments = ["two words 中文"].map(OsString::from);
            assert!(ArtisanFacade::call("say-hi", &[], output).await.is_err());
            assert!(
                ArtisanFacade::call("unknown-command", &[], output)
                    .await
                    .is_err()
            );
            assert_eq!(ArtisanFacade::callSilently("say-hi", &arguments).await?, 7);
            let mut help = Output::default();
            assert_eq!(
                ArtisanFacade::call("say-hi", &["--help".into()], &mut help).await?,
                0
            );
            assert!(help.stdout.contains("<name>"));
            assert_eq!(
                ArtisanFacade::call("say-hi", &["--quiet".into(), "--help".into()], output).await?,
                0
            );
            let mut output_json = Output::default();
            let help_arguments = ["inspire", "--format=json"].map(OsString::from);
            assert_eq!(
                ArtisanFacade::call("help", &help_arguments, &mut output_json).await?,
                0
            );
            let metadata: serde_json::Value = serde_json::from_str(&output_json.stdout).unwrap();
            assert_eq!(metadata["name"], "inspire");
            assert_eq!(metadata["implemented"], true);
            ArtisanFacade::call("say-hi", &arguments, output).await
        })
    }
}

struct QueuedArtisan(CommandDefinition);

impl Command for QueuedArtisan {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            use openrust::queue::{DispatchResult, QueueError};

            let arguments = ["a value 中文"].map(OsString::from);
            assert_eq!(
                ArtisanFacade::queue("say-hi", &arguments)
                    .onConnection("sync")
                    .onQueue("commands")
                    .dispatch()
                    .await
                    .unwrap(),
                DispatchResult::Completed
            );
            assert!(matches!(
                ArtisanFacade::queue("missing:command", &[])
                    .onConnection("sync")
                    .dispatch()
                    .await,
                Err(QueueError::Execution { .. })
            ));
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt as _;
                let invalid = [OsString::from_vec(vec![0xff])];
                assert!(matches!(
                    ArtisanFacade::queue("say-hi", &invalid).dispatch().await,
                    Err(QueueError::InvalidPayload)
                ));
            }
            Ok(0)
        })
    }
}

#[tokio::test]
async fn artisan_facade_preserves_arguments_errors_exit_status_and_output() {
    assert!(
        ArtisanFacade::callSilently("list", &[])
            .await
            .unwrap_err()
            .to_string()
            .contains("No active Artisan application")
    );
    let app = Application::configure(std::env::current_dir().unwrap())
        .withCommands(|commands| {
            commands.add_command(Box::new(Greet(CommandDefinition::new(
                clap::Command::new("greet")
                    .visible_alias("say-hi")
                    .arg(Arg::new("name").required(true)),
            ))))?;
            commands.add_command(Box::new(NestedArtisan(CommandDefinition::new(
                clap::Command::new("nested"),
            ))))?;
            commands.add_command(Box::new(QueuedArtisan(CommandDefinition::new(
                clap::Command::new("queued-probe"),
            ))))
        })
        .withRouting(Routing::default().commands(application_console::register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app, Artisan::new());
    let mut output = Output::default();
    assert_eq!(kernel.handle(&["nested".into()], &mut output).await, 7);
    assert_eq!(output.stdout, "Hello two words 中文\n");
    assert!(output.stderr.is_empty());

    let mut queued = Output::default();
    assert_eq!(
        kernel.handle(&["queued-probe".into()], &mut queued).await,
        0
    );
    assert!(queued.stdout.is_empty() && queued.stderr.is_empty());
}

static RELOAD_STEPS: AtomicUsize = AtomicUsize::new(0);

struct ReloadStep(CommandDefinition, usize, i32);

impl Command for ReloadStep {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            RELOAD_STEPS.fetch_or(self.1, Ordering::Relaxed);
            Ok(self.2)
        })
    }
}

struct ReloadProbeServiceProvider;

impl ServiceProvider for ReloadProbeServiceProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.reloads(app, "probe:replaced", None);
            self.reloads(app, "probe:fail", Some("reload-probe"));
            self.reloads(app, "probe:next", Some("next"));
            self.optimizes(app, Some("probe:fail"), Some("probe:next"), None);
            Ok(())
        })
    }
}

#[tokio::test]
async fn artisan_uses_application_provider_tasks_and_continues_after_nonzero_status() {
    for (with_provider, command, except, expected) in [
        (true, "reload", "queue, schedule:interrupt", 3),
        (
            true,
            "reload",
            "queue, schedule, reload-probe, probe:next, next",
            0,
        ),
        (false, "reload", "queue, schedule", 0),
        (true, "optimize", "config, event:cache, routes, views", 1),
        (
            true,
            "optimize:clear",
            "config, cache, compiled, events, routes, view:clear",
            2,
        ),
        (
            true,
            "optimize",
            "config, events, routes, views, reload-probe",
            0,
        ),
    ] {
        RELOAD_STEPS.store(0, Ordering::Relaxed);
        let mut builder =
            Application::configure(std::env::current_dir().unwrap()).withCommands(|artisan| {
                for (name, step, status) in [("probe:fail", 1, 7), ("probe:next", 2, 0)] {
                    artisan.add_command(Box::new(ReloadStep(
                        CommandDefinition::new(clap::Command::new(name)),
                        step,
                        status,
                    )))?;
                }
                Ok(())
            });
        if with_provider {
            builder = builder.withProviders(vec![Box::new(ReloadProbeServiceProvider)]);
        }
        let app = builder.create().unwrap();
        let status = app
            .handleCommand(&[command, "-e", except].map(OsString::from))
            .await
            .unwrap();
        assert_eq!(status, 0);
        assert_eq!(RELOAD_STEPS.load(Ordering::Relaxed), expected);
    }
}

#[tokio::test]
async fn raw_help_includes_the_registered_commands_long_help() {
    let app = Application::configure(std::env::current_dir().unwrap())
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app, Artisan::new());
    kernel
        .register_command(Box::new(Greet(CommandDefinition::new(
            clap::Command::new("greet")
                .visible_alias("say-hi")
                .about("Greet someone")
                .long_about("Greet someone and describe the full workflow.")
                .arg(
                    Arg::new("name")
                        .required(true)
                        .help("Name")
                        .long_help("Name as it should appear in the greeting."),
                ),
        ))))
        .unwrap();
    let mut direct = Output::default();
    assert_eq!(
        kernel
            .handle(&["say-hi", "--help"].map(OsString::from), &mut direct)
            .await,
        0
    );
    let mut raw = Output::default();
    assert_eq!(
        kernel
            .handle(&["help", "say-hi", "--raw"].map(OsString::from), &mut raw,)
            .await,
        0
    );
    assert_eq!(direct.stdout, raw.stdout);
    assert!(raw.stdout.contains("describe the full workflow"));
    assert!(raw.stdout.contains("as it should appear in the greeting"));
}

#[tokio::test]
async fn artisan_reports_dispatch_errors_and_keeps_console_rendering() {
    use openrust::foundation::configuration::ReportDecision;
    use std::sync::Arc;

    let reports = Arc::new(AtomicUsize::new(0));
    let observed = reports.clone();
    let application = Application::configure(std::env::current_dir().unwrap())
        .withExceptions(move |exceptions| {
            exceptions.report::<ConsoleError>(move |error| {
                assert!(error.to_string().contains("not implemented yet"));
                observed.fetch_add(1, Ordering::Relaxed);
                ReportDecision::Stop
            });
            exceptions.render::<ConsoleError>(|_, _| panic!("HTTP renderer called by Artisan"));
            exceptions.respond(|_, _, _| panic!("HTTP responder called by Artisan"));
        })
        .create()
        .unwrap();
    let mut artisan = Artisan::new();
    artisan.add_planned(clap::Command::new("probe")).unwrap();
    let mut kernel = Kernel::new(application, artisan);
    let mut output = Output::default();
    assert_eq!(kernel.handle(&["probe".into()], &mut output).await, 1);
    assert_eq!(output.stderr, "Command 'probe' is not implemented yet.\n");
    assert_eq!(reports.load(Ordering::Relaxed), 1);

    let mut output = Output::default();
    assert_eq!(kernel.handle(&["--invalid".into()], &mut output).await, 1);
    assert!(output.stderr.contains("--invalid"));
    assert_eq!(reports.load(Ordering::Relaxed), 1);
}

#[test]
fn facades_keep_concurrent_applications_isolated() {
    use std::sync::Barrier;

    static WEB: Barrier = Barrier::new(2);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            check_facade_application(
                || {
                    WEB.wait();
                    Route::get("/first", || {
                        view("welcome")
                            .with("name", "first")
                            .with("version", "test")
                            .with("repository", Application::REPOSITORY)
                            .with("language", "en")
                    })
                },
                || {
                    ArtisanFacade::command("probe", |output| output.comment("first"))?
                        .purpose("First application")
                },
                "/first",
                "/second",
                "first",
                "First application",
            );
        });
        let second = scope.spawn(|| {
            check_facade_application(
                || {
                    WEB.wait();
                    Route::get("/second", || {
                        view("welcome")
                            .with("name", "second")
                            .with("version", "test")
                            .with("repository", Application::REPOSITORY)
                            .with("language", "en")
                    })
                },
                || {
                    ArtisanFacade::command("probe", |output| output.comment("second"))?
                        .purpose("Second application")
                },
                "/second",
                "/first",
                "second",
                "Second application",
            );
        });
        first.join().unwrap();
        second.join().unwrap();
    });
}

fn check_facade_application(
    web: fn() -> Result<(), RoutingError>,
    commands: fn() -> Result<(), ConsoleError>,
    own_route: &str,
    other_route: &str,
    message: &str,
    purpose: &str,
) {
    let base = std::env::current_dir().unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let app = Application::configure(base.clone())
                .withRouting(Routing::default().web(web))
                .create()
                .unwrap();
            let mut http = http::Kernel::new(app);
            http.bootstrap().await.unwrap();
            for (path, status) in [
                (own_route, StatusCode::OK),
                (other_route, StatusCode::NOT_FOUND),
            ] {
                let (parts, body) = axum::http::Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .unwrap()
                    .into_parts();
                let mut request = Request::from_parts(parts, body);
                assert_eq!(http.handle(&mut request).await.status(), status);
            }
            assert!(matches!(
                Route::get("/outside", || view("welcome")),
                Err(RoutingError::NoRegistrationScope)
            ));

            let app = Application::configure(base)
                .withRouting(Routing::default().commands(commands))
                .create()
                .unwrap();
            let mut console = Kernel::new(app, Artisan::new());
            let mut output = Output::default();
            assert_eq!(
                console
                    .handle(&[OsString::from("probe")], &mut output)
                    .await,
                0
            );
            assert_eq!(output.stdout, format!("{message}\n"));
            let mut help = Output::default();
            let args = ["help", "probe"].map(OsString::from);
            assert_eq!(console.handle(&args, &mut help).await, 0);
            assert!(help.stdout.contains(purpose));
            assert!(ArtisanFacade::command("outside", |_| {}).is_err());
        });
}

#[tokio::test]
async fn facade_registration_errors_reach_the_kernel() {
    let base = std::env::current_dir().unwrap();
    let app = Application::configure(base.clone())
        .withRouting(
            Routing::default().web(|| Route::get("/items/{id?}/{id?}", || view("welcome"))),
        )
        .create()
        .unwrap();
    let mut http = http::Kernel::new(app);
    assert!(matches!(
        http.bootstrap().await,
        Err(BootstrapError::RoutingFailed { .. })
    ));
    assert!(matches!(
        Route::get("/outside", || view("welcome")),
        Err(RoutingError::NoRegistrationScope)
    ));

    let app = Application::configure(base)
        .withCommands(|artisan| {
            artisan.add_planned(clap::Command::new("existing").visible_alias("taken"))
        })
        .withRouting(
            Routing::default()
                .commands(|| ArtisanFacade::command("taken", |_| {})?.purpose("Alias collision")),
        )
        .create()
        .unwrap();
    let mut console = Kernel::new(app, Artisan::new());
    let error = console.bootstrap().await.unwrap_err();
    assert!(matches!(
        error,
        BootstrapError::CommandRegistrationFailed { .. }
    ));
    assert!(error.to_string().contains("already in use"));
    assert!(ArtisanFacade::command("outside", |_| {}).is_err());
}

#[tokio::test]
async fn registered_command_uses_the_same_definition_in_cli_and_programmatic_calls() {
    static PHASE: AtomicUsize = AtomicUsize::new(0);
    let app = Application::configure(std::env::current_dir().unwrap())
        .withMiddleware(|_| panic!("console must not configure HTTP middleware"))
        .withRouting(
            Routing::new(|_| {
                assert_eq!(PHASE.fetch_add(1, Ordering::Relaxed), 0);
                Ok(openrust::routing::Router::new())
            })
            .commands(|| {
                assert_eq!(PHASE.fetch_add(1, Ordering::Relaxed), 2);
                Ok(())
            }),
        )
        .withCommands(|_| {
            assert_eq!(PHASE.fetch_add(1, Ordering::Relaxed), 1);
            Ok(())
        })
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app, Artisan::new());
    kernel
        .register_command(Box::new(Greet(CommandDefinition::new(
            clap::Command::new("greet")
                .visible_alias("say-hi")
                .arg(Arg::new("name").required(true)),
        ))))
        .unwrap();

    let args = ["say-hi", "Alice"].map(OsString::from);
    let mut output = Output::default();
    assert_eq!(kernel.handle(&args, &mut output).await, 7);
    assert_eq!(output.stdout, "Hello Alice\n");
    assert!(output.stderr.is_empty());

    let input = kernel
        .all()
        .into_iter()
        .find(|definition| definition.name() == "greet")
        .unwrap()
        .parse(&[OsString::from("Bob")])
        .unwrap();
    assert_eq!(kernel.call("say-hi", &input, None).await.unwrap(), 7);
    assert_eq!(kernel.output(), "Hello Bob\n");
    assert!(kernel.output().is_empty());

    let unrelated = CommandDefinition::new(clap::Command::new("greet"))
        .parse(&[])
        .unwrap();
    assert!(kernel.call("greet", &unrelated, None).await.is_err());
    assert_eq!(PHASE.load(Ordering::Relaxed), 3);
}
