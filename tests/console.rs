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
    support::facades::{Artisan as ArtisanFacade, Route},
    view::view,
};

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
                    Route::get("/first", || view("welcome"))
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
                    Route::get("/second", || view("welcome"))
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
