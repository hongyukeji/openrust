use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::ServiceKey,
    contracts::{
        console::Kernel as _,
        container::{BindingResolutionError, Container as _},
        http::Kernel as _,
    },
    foundation::{Application, configuration::Routing, console, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use std::{
    io::{Read as _, Write as _},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
#[derive(Default)]
struct Stats {
    constructed: AtomicUsize,
    after: AtomicUsize,
}
struct Probe(String);
struct Probes;
impl Facade for Probes {
    type Service = Probe;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Probe>()
    }
}
struct Provider {
    name: &'static str,
    fail: bool,
    stats: Arc<Stats>,
}
impl ServiceProvider for Provider {
    fn register(&mut self, app: &Application) -> ProviderResult {
        use openrust::contracts::foundation::Application as _;
        let c = app.container();
        let name = self.name;
        let stats = self.stats.clone();
        c.scoped::<Probe, _>(move |_| {
            stats.constructed.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(Probe(name.into())))
        });
        c.extend::<Probe, _>(ServiceKey::typed::<Probe>(), |value, _| {
            Ok(Arc::new(Probe(format!("{}:one", value.0))))
        })?;
        let fail = AtomicBool::new(self.fail);
        c.extend::<Probe, _>(ServiceKey::typed::<Probe>(), move |value, _| {
            if fail.swap(false, Ordering::SeqCst) {
                Err(BindingResolutionError::NotBound {
                    service: "extension-failure".into(),
                })
            } else {
                Ok(Arc::new(Probe(format!("{}:two", value.0))))
            }
        })?;
        let stats = self.stats.clone();
        c.afterResolving(Some(ServiceKey::typed::<Probe>()), move |event, app| {
            assert!(event.value::<Probe>().unwrap().0.ends_with(":one:two"));
            stats.after.fetch_add(1, Ordering::SeqCst);
            app.extend::<Probe, _>(event.key().clone(), |value, _| {
                Ok(Arc::new(Probe(format!("{}:cached", value.0))))
            })
        })?;
        Ok(())
    }
}
fn result() -> Result<String, BindingResolutionError> {
    let first = Probes::getFacadeRoot()?;
    let second = Probes::getFacadeRoot()?;
    assert!(!Arc::ptr_eq(&first, &second));
    Ok(format!("{}|{}", first.0, second.0))
}
fn web() -> Result<(), RoutingError> {
    Route::get("/extend", request)?;
    Ok(())
}
fn request(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let text = result().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::new(StatusCode::OK, Body::from(text)))
    })
}
struct ProbeCommand(CommandDefinition);
impl Command for ProbeCommand {
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
            output
                .stdout
                .push_str(&result().map_err(|error| ConsoleError::new(error.to_string()))?);
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ProbeCommand(CommandDefinition::new(
        clap::Command::new("extend:probe"),
    ))))
}
fn application(name: &'static str, fail: bool, stats: Arc<Stats>) -> Application {
    Application::configure(std::env::current_dir().unwrap())
        .withProviders(vec![Box::new(Provider { name, fail, stats })])
        .withCommands(commands)
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|_, _| true);
        })
        .withRouting(Routing::default().web(web))
        .create()
        .unwrap()
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut incoming = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut incoming).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}
async fn server(app: Application) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
fn send(address: std::net::SocketAddr, status: u16) -> String {
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream.set_write_timeout(Some(timeout)).unwrap();
    write!(
        stream,
        "GET /extend HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status}")),
        "{response}"
    );
    response.split_once("\r\n\r\n").unwrap().1.to_owned()
}

#[tokio::test]
async fn actual_http_decorators_run_before_callbacks_and_retry_failed_construction() {
    let a = Arc::new(Stats::default());
    let b = Arc::new(Stats::default());
    let ((first, x), (second, y)) = tokio::join!(
        server(application("A", false, a.clone())),
        server(application("B", true, b.clone()))
    );
    let bodies = tokio::task::spawn_blocking(move || {
        let first_round = std::thread::scope(|scope| {
            let one = scope.spawn(|| send(first, 200));
            let two = scope.spawn(|| send(second, 500));
            (one.join().unwrap(), two.join().unwrap())
        });
        (first_round, send(first, 200), send(second, 200))
    })
    .await
    .unwrap();
    assert_eq!(bodies.0.0, "A:one:two|A:one:two:cached");
    assert!(bodies.0.1.contains("Internal Server Error"));
    assert_eq!(bodies.1, "A:one:two|A:one:two:cached");
    assert_eq!(bodies.2, "B:one:two|B:one:two:cached");
    assert_eq!(a.constructed.load(Ordering::SeqCst), 2);
    assert_eq!(b.constructed.load(Ordering::SeqCst), 2);
    assert_eq!(a.after.load(Ordering::SeqCst), 2);
    assert_eq!(b.after.load(Ordering::SeqCst), 1);
    x.abort();
    y.abort();
}
#[tokio::test]
async fn artisan_extender_failure_is_reported_and_the_next_command_gets_a_fresh_scope() {
    let stats = Arc::new(Stats::default());
    let mut kernel = console::Kernel::new(
        application("CLI", true, stats.clone()),
        openrust::console::Application::new(),
    );
    for expected in [1, 0, 0] {
        let mut output = Output::default();
        assert_eq!(
            kernel.handle(&["extend:probe".into()], &mut output).await,
            expected
        );
        if expected == 0 {
            assert_eq!(output.stdout, "CLI:one:two|CLI:one:two:cached");
            assert!(output.stderr.is_empty());
        } else {
            assert!(output.stderr.contains("extension-failure"));
            assert!(output.stdout.is_empty());
        }
    }
    assert_eq!(stats.constructed.load(Ordering::SeqCst), 3);
    assert_eq!(stats.after.load(Ordering::SeqCst), 2);
}
