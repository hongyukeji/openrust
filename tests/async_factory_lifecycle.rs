use openrust::{
    config::{ConfigError, Environment, Value},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::{FactoryFuture, ServiceKey},
    contracts::{
        console::Kernel as _,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        http::Kernel as _,
        support::DeferrableProvider,
    },
    foundation::{Application, configuration::Routing, console, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        DeferredProvider, ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-async-factory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([(
        "app".into(),
        serde_json::json!({"host":"127.0.0.1","port":0}),
    )]))
}
#[derive(Default)]
struct Counts {
    client: AtomicUsize,
    worker: AtomicUsize,
    before: AtomicUsize,
    register: AtomicUsize,
    boot: AtomicUsize,
}
struct Client {
    name: String,
    attempt: usize,
}
struct Worker {
    client: Arc<Client>,
    sequence: usize,
}
struct Clients;
impl Facade for Clients {
    type Service = Client;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Client>()
    }
}
struct Label(&'static str);
struct Labels;
impl Facade for Labels {
    type Service = Label;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Label>()
    }
}
struct DropProbe(Arc<Control>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        let value = Labels::getFacadeRoot()
            .map(|label| label.0)
            .unwrap_or("missing");
        self.0.drops.lock().unwrap().push(value);
    }
}
struct Workers;
impl Facade for Workers {
    type Service = Worker;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Worker>()
    }
}
#[derive(Default)]
struct Control {
    entered: Notify,
    release: Notify,
    pending: Mutex<Option<FactoryFuture<'static, Worker>>>,
    drops: Mutex<Vec<&'static str>>,
    other: Mutex<Option<Arc<Application>>>,
}
struct Controls;
impl Facade for Controls {
    type Service = Control;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Control>()
    }
}
struct ClientProvider {
    name: String,
    counts: Arc<Counts>,
    fail_first: bool,
}
impl DeferrableProvider for ClientProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Client>()]
    }
}
impl ServiceProvider for ClientProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.counts.register.fetch_add(1, Ordering::SeqCst);
            let (name, counts, fail) = (self.name.clone(), self.counts.clone(), self.fail_first);
            app.container()
                .singletonAsync::<Client, _>(move |_| {
                    let (name, counts) = (name.clone(), counts.clone());
                    Box::pin(async move {
                        let attempt = counts.client.fetch_add(1, Ordering::SeqCst) + 1;
                        tokio::task::yield_now().await;
                        if fail && attempt == 1 {
                            return Err(BindingResolutionError::FactoryFailed {
                                service: "client",
                                source: std::io::Error::other("private connection detail").into(),
                            });
                        }
                        Ok(Arc::new(Client { name, attempt }))
                    })
                })
                .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.counts.boot.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}
struct WorkerProvider {
    counts: Arc<Counts>,
    pause: Option<Arc<Control>>,
}
impl ServiceProvider for WorkerProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .scoped::<Label, _>(|_| Ok(Arc::new(Label("root"))))?;
            let counts = self.counts.clone();
            app.container()
                .beforeResolving(Some(ServiceKey::typed::<Worker>()), move |_, _| {
                    counts.before.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })?;
            let (counts, pause) = (self.counts.clone(), self.pause.clone());
            app.container()
                .scopedAsync::<Worker, _>(move |c| {
                    let (counts, pause) = (counts.clone(), pause.clone());
                    Box::pin(async move {
                        let sequence = counts.worker.fetch_add(1, Ordering::SeqCst) + 1;
                        let _probe = pause.as_ref().map(|pause| DropProbe(pause.clone()));
                        if let Some(pause) = pause {
                            pause.entered.notify_one();
                            pause.release.notified().await;
                        }
                        let client = c.makeAsync::<Client>().await?;
                        let via_facade = Clients::getFacadeRootAsync().await?;
                        assert!(Arc::ptr_eq(&client, &via_facade));
                        tokio::task::yield_now().await;
                        Ok(Arc::new(Worker { client, sequence }))
                    })
                })
                .await?;
            Ok(())
        })
    }
}
fn json(worker: &Worker) -> String {
    serde_json::json!({"name":worker.client.name,"attempt":worker.client.attempt,"sequence":worker.sequence}).to_string()
}
fn internal(source: BindingResolutionError) -> HttpError {
    HttpError::Internal {
        source: source.into(),
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let a = Workers::getFacadeRootAsync().await.map_err(internal)?;
        let b = Workers::getFacadeRootAsync().await.map_err(internal)?;
        assert!(Arc::ptr_eq(&a, &b));
        Ok(Response::new(StatusCode::OK, Body::from(json(&a))))
    })
}
fn stash(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Labels::swap(Arc::new(Label("a"))).map_err(internal)?;
        let control = Controls::getFacadeRoot().map_err(internal)?;
        let mut pending: FactoryFuture<'static, Worker> = Box::pin(Workers::getFacadeRootAsync());
        tokio::select! {value=&mut pending=>panic!("worker unexpectedly ready: {}",value.unwrap().sequence),_=control.entered.notified()=>{}}
        *control.pending.lock().unwrap() = Some(pending);
        Ok(Response::new(StatusCode::OK, Body::from("stored")))
    })
}
fn resume(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Labels::swap(Arc::new(Label("b"))).map_err(internal)?;
        let control = Controls::getFacadeRoot().map_err(internal)?;
        let pending = control.pending.lock().unwrap().take().unwrap();
        control.release.notify_one();
        assert!(matches!(
            pending.await,
            Err(BindingResolutionError::FacadeNotSet)
        ));
        assert_eq!(*control.drops.lock().unwrap(), ["missing"]);
        let worker = Workers::getFacadeRootAsync().await.map_err(internal)?;
        assert_eq!(worker.sequence, 2);
        Ok(Response::new(StatusCode::OK, Body::from(json(&worker))))
    })
}
fn cancel_live(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Labels::swap(Arc::new(Label("a"))).map_err(internal)?;
        let control = Controls::getFacadeRoot().map_err(internal)?;
        let mut pending: FactoryFuture<'static, Worker> = Box::pin(Workers::getFacadeRootAsync());
        tokio::select! {value=&mut pending=>panic!("unexpected {}",value.unwrap().sequence),_=control.entered.notified()=>{}}
        *control.pending.lock().unwrap() = Some(pending);
        let other = control.other.lock().unwrap().take().unwrap();
        other.make::<Killer>().await.map_err(internal)?;
        assert_eq!(*control.drops.lock().unwrap(), ["a"]);
        assert_eq!(Labels::getFacadeRoot().map_err(internal)?.0, "a");
        Ok(Response::new(StatusCode::OK, Body::from("cancelled")))
    })
}
struct Killer;
fn routes() -> Result<(), RoutingError> {
    Route::get("/worker", read)?;
    Route::get("/stash", stash)?;
    Route::get("/resume", resume)?;
    Route::get("/cancel", cancel_live)
}
struct Probe(CommandDefinition);
impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let worker = app
                .make::<Worker>()
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.stdout.push_str(&json(&worker));
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
        "async:worker",
    )))))
}
fn application(
    root: &Root,
    name: &str,
    counts: Arc<Counts>,
    fail_first: bool,
    pause: Option<Arc<Control>>,
) -> Application {
    let (label, client_counts) = (name.to_owned(), counts.clone());
    let app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(WorkerProvider {
                counts,
                pause: pause.clone(),
            }),
            Box::new(DeferredProvider::new(move || ClientProvider {
                name: label.clone(),
                counts: client_counts.clone(),
                fail_first,
            })),
        ])
        .withCommands(commands)
        .withRouting(Routing::default().web(routes))
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|_, _| true);
        })
        .create()
        .unwrap();
    if let Some(control) = pause {
        app.container().instance(control).unwrap();
    }
    app
}
async fn transport(
    axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut request = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}
async fn server(app: Application) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}
async fn send(address: std::net::SocketAddr, path: &str, status: u16) -> String {
    use std::io::{Read as _, Write as _};
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let timeout = std::time::Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").unwrap();
        let mut result = String::new();
        stream.read_to_string(&mut result).unwrap();
        assert!(result.starts_with(&format!("HTTP/1.1 {status}")), "{result}");
        result.split_once("\r\n\r\n").unwrap().1.to_owned()
    }).await.unwrap()
}
#[tokio::test]
async fn real_http_automatically_loads_deferred_factory_dependencies_and_isolates_scopes_and_apps()
{
    let root = Root::new();
    let a = Arc::new(Counts::default());
    let b = Arc::new(Counts::default());
    let failed = Arc::new(Counts::default());
    let (first, x) = server(application(&root, "a", a.clone(), false, None)).await;
    let (second, y) = server(application(&root, "b", b.clone(), false, None)).await;
    let (bad, z) = server(application(&root, "retry", failed.clone(), true, None)).await;
    for counts in [&a, &b, &failed] {
        assert_eq!(counts.register.load(Ordering::SeqCst), 0);
        assert_eq!(counts.client.load(Ordering::SeqCst), 0);
    }
    assert!(send(first, "/worker", 200).await.contains("\"sequence\":1"));
    assert!(send(first, "/worker", 200).await.contains("\"sequence\":2"));
    assert!(
        send(second, "/worker", 200)
            .await
            .contains("\"name\":\"b\"")
    );
    let failure = send(bad, "/worker", 500).await;
    assert!(!failure.contains("private connection detail"));
    let retry = send(bad, "/worker", 200).await;
    assert!(retry.contains("\"attempt\":2"));
    assert_eq!(a.client.load(Ordering::SeqCst), 1);
    assert_eq!(a.before.load(Ordering::SeqCst), 2);
    assert_eq!(failed.register.load(Ordering::SeqCst), 1);
    assert_eq!(failed.boot.load(Ordering::SeqCst), 1);
    for task in [x, y, z] {
        task.abort();
        let _ = task.await;
    }
}
#[tokio::test]
async fn artisan_factory_dependencies_retry_failures_and_reset_command_scopes() {
    let root = Root::new();
    let counts = Arc::new(Counts::default());
    let app = application(&root, "console", counts.clone(), true, None);
    let mut kernel = console::Kernel::new(app, openrust::console::Application::new());
    let args = ["async:worker".into()];
    for (expected, sequence) in [(1, 1), (0, 2), (0, 3)] {
        let mut output = Output::default();
        assert_eq!(kernel.handle(&args, &mut output).await, expected);
        if expected == 0 {
            assert!(output.stdout.contains(&format!("\"sequence\":{sequence}")));
        }
    }
    assert_eq!(counts.client.load(Ordering::SeqCst), 2);
    assert_eq!(counts.register.load(Ordering::SeqCst), 1);
    assert_eq!(counts.boot.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn a_retained_facade_future_cannot_resume_inside_another_http_requests_cache() {
    let root = Root::new();
    let counts = Arc::new(Counts::default());
    let control = Arc::new(Control::default());
    let (address, task) = server(application(
        &root,
        "retained",
        counts.clone(),
        false,
        Some(control),
    ))
    .await;
    assert!(send(address, "/stash", 200).await.contains("stored"));
    assert_eq!(counts.client.load(Ordering::SeqCst), 0);
    assert!(
        send(address, "/resume", 200)
            .await
            .contains("\"sequence\":2")
    );
    assert_eq!(counts.client.load(Ordering::SeqCst), 1);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn application_make_binds_its_own_facades_without_inventing_a_request_scope() {
    let root = Root::new();
    let mut a = application(&root, "a", Arc::new(Counts::default()), false, None);
    let mut b = application(&root, "b", Arc::new(Counts::default()), false, None);
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    let first = a.make::<Worker>().await.unwrap();
    let second = b.make::<Worker>().await.unwrap();
    assert_eq!(first.client.name, "a");
    assert_eq!(second.client.name, "b");
    assert!(Arc::ptr_eq(&first, &a.make::<Worker>().await.unwrap()));
}

#[tokio::test]
async fn cancelling_a_live_factory_restores_its_application_during_cleanup() {
    let root = Root::new();
    let control = Arc::new(Control::default());
    let mut other = Application::configure(root.0.clone()).create().unwrap();
    other.boot().await.unwrap();
    let owned = control.clone();
    other
        .container()
        .bindAsync::<Killer, _>(move |_| {
            let control = owned.clone();
            Box::pin(async move {
                assert!(matches!(
                    Labels::getFacadeRoot(),
                    Err(BindingResolutionError::NotBound { .. })
                ));
                let retired = control.pending.lock().unwrap().take().unwrap();
                drop(retired);
                assert!(matches!(
                    Labels::getFacadeRoot(),
                    Err(BindingResolutionError::NotBound { .. })
                ));
                Ok(Arc::new(Killer))
            })
        })
        .await
        .unwrap();
    *control.other.lock().unwrap() = Some(Arc::new(other));
    let counts = Arc::new(Counts::default());
    let (address, task) = server(application(
        &root,
        "live",
        counts.clone(),
        false,
        Some(control),
    ))
    .await;
    assert_eq!(send(address, "/cancel", 200).await, "cancelled");
    assert_eq!(counts.client.load(Ordering::SeqCst), 0);
    task.abort();
    let _ = task.await;
}
