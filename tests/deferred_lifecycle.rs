use openrust::{
    config::{ConfigError, Environment, Value},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::ServiceKey,
    contracts::{
        console::Kernel as _,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
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
        let p = std::env::temp_dir().join(format!(
            "openrust-deferred-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
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
        serde_json::json!({"host":"127.0.0.1","port":0,"name":"deferred"}),
    )]))
}
struct Service(u32);
struct Lazy {
    value: u32,
    trace: Arc<Mutex<Vec<&'static str>>>,
    pause: Option<Arc<(Notify, Notify)>>,
    fail: bool,
}
impl ServiceProvider for Lazy {
    #[allow(non_snake_case)]
    fn isDeferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Service>()]
    }
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("lazy.register");
            app.container().instance(Arc::new(Service(self.value)))?;
            app.container()
                .alias(ServiceKey::typed::<Service>(), "lazy")?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("lazy.boot.begin");
            assert_eq!(app.make::<Service>().await?.0, self.value);
            assert_eq!(ServiceFacade::getFacadeRoot()?.0, self.value);
            if let Some(pause) = &self.pause {
                pause.0.notify_one();
                pause.1.notified().await;
            }
            if self.fail {
                return Err(std::io::Error::other("private boot detail").into());
            }
            self.trace.lock().unwrap().push("lazy.boot.end");
            Ok(())
        })
    }
}
fn application(
    root: &Root,
    value: u32,
    trace: Arc<Mutex<Vec<&'static str>>>,
    pause: Option<Arc<(Notify, Notify)>>,
    fail: bool,
) -> Application {
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(Lazy {
            value,
            trace,
            pause,
            fail,
        })])
        .withRouting(Routing::default().web(web))
        .create()
        .unwrap()
}
struct ServiceFacade;
impl Facade for ServiceFacade {
    type Service = Service;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Service>()
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let service = ServiceFacade::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        Ok(Response::new(
            StatusCode::OK,
            Body::from(service.0.to_string()),
        ))
    })
}
fn web() -> Result<(), RoutingError> {
    Route::get("/concurrent", concurrent_facade_read)?;
    Route::get("/deferred", read)
}
struct ProbeCommand(CommandDefinition);
impl Command for ProbeCommand {
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
            let service = app
                .make::<Service>()
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output.stdout.push_str(&service.0.to_string());
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ProbeCommand(CommandDefinition::new(
        clap::Command::new("deferred:probe"),
    ))))
}

#[tokio::test]
async fn first_lookup_waits_for_boot_and_concurrent_readers_share_one_attempt() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let pause = Arc::new((Notify::new(), Notify::new()));
    let mut app = application(&root, 42, trace.clone(), Some(pause.clone()), false);
    app.boot().await.unwrap();
    assert!(trace.lock().unwrap().is_empty());
    assert!(app.isDeferredService(ServiceKey::typed::<Service>()));
    assert!(app.container().bound::<Service>());
    assert!(matches!(
        app.container().make::<Service>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    let mut first = Box::pin(app.make::<Service>());
    tokio::select! {value=&mut first=>panic!("early value {}",value.unwrap().0),_=pause.0.notified()=>{}}
    assert!(matches!(
        app.container().make::<Service>(),
        Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
    ));
    let mut second = Box::pin(app.get::<Service>("lazy"));
    std::future::poll_fn(|cx| {
        assert!(second.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    pause.1.notify_one();
    let (a, b) = tokio::join!(first, second);
    assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
    assert_eq!(
        *trace.lock().unwrap(),
        ["lazy.register", "lazy.boot.begin", "lazy.boot.end"]
    );
}
#[tokio::test]
async fn bootstrap_first_use_joins_the_boot_tail_and_explicit_values_skip_unused_loaders() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut app = application(&root, 3, trace.clone(), None, false);
    assert_eq!(app.make::<Service>().await.unwrap().0, 3);
    assert_eq!(*trace.lock().unwrap(), ["lazy.register"]);
    app.boot().await.unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "lazy.register",
            "lazy.boot.begin",
            "lazy.boot.end",
            "lazy.boot.begin",
            "lazy.boot.end"
        ]
    );
    app.loadDeferredProviders().await.unwrap();
    assert_eq!(trace.lock().unwrap().len(), 5);
    let other = application(&root, 7, Arc::new(Mutex::new(Vec::new())), None, false);
    other.container().instance(Arc::new(Service(9))).unwrap();
    assert_eq!(other.make::<Service>().await.unwrap().0, 9);
}
#[tokio::test]
async fn boot_failure_does_not_publish_a_ready_service_or_retry_its_partial_boot() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut app = application(&root, 3, trace.clone(), None, true);
    app.boot().await.unwrap();
    for _ in 0..2 {
        assert!(matches!(
            app.make::<Service>().await,
            Err(BindingResolutionError::DeferredProviderFailed { .. })
        ));
    }
    assert_eq!(trace.lock().unwrap().len(), 2);
    assert!(app.container().make::<Service>().is_err());
}
#[tokio::test]
async fn cancellation_marks_the_in_flight_provider_unready_and_releases_waiters() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let pause = Arc::new((Notify::new(), Notify::new()));
    let mut app = application(&root, 3, trace, Some(pause.clone()), false);
    app.boot().await.unwrap();
    let mut first = Box::pin(app.make::<Service>());
    tokio::select! {value=&mut first=>panic!("early value {}",value.unwrap().0),_=pause.0.notified()=>{}}
    let mut waiter = Box::pin(app.make::<Service>());
    std::future::poll_fn(|cx| {
        assert!(waiter.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(first);
    assert!(matches!(
        waiter.await,
        Err(BindingResolutionError::DeferredProviderFailed { .. })
    ));
}
#[tokio::test]
async fn artisan_loads_remaining_providers_and_resolves_the_ready_service() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(Lazy {
            value: 5,
            trace: trace.clone(),
            pause: None,
            fail: false,
        })])
        .withCommands(commands)
        .create()
        .unwrap();
    let mut kernel = console::Kernel::new(app, openrust::console::Application::new());
    let mut output = Output::default();
    let args = ["deferred:probe".into()];
    assert_eq!(kernel.handle(&args, &mut output).await, 0);
    assert!(output.stdout.contains('5'));
    assert_eq!(trace.lock().unwrap().len(), 3);
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
        let timeout=std::time::Duration::from_secs(5);let mut stream=std::net::TcpStream::connect_timeout(&address,timeout).unwrap();stream.set_read_timeout(Some(timeout)).unwrap();
        write!(stream,"GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").unwrap();
        let mut result=String::new();stream.read_to_string(&mut result).unwrap();assert!(result.starts_with(&format!("HTTP/1.1 {status}")),"{result}");result.split_once("\r\n\r\n").unwrap().1.to_owned()
    }).await.unwrap()
}
#[tokio::test]
async fn actual_http_loads_on_demand_keeps_apps_isolated_and_sanitizes_boot_failures() {
    let root = Root::new();
    let a = Arc::new(Mutex::new(Vec::new()));
    let b = Arc::new(Mutex::new(Vec::new()));
    let bad = Arc::new(Mutex::new(Vec::new()));
    let (first, x) = server(application(&root, 3, a.clone(), None, false)).await;
    let (second, y) = server(application(&root, 9, b.clone(), None, false)).await;
    let (failed, z) = server(application(&root, 7, bad.clone(), None, true)).await;
    assert!(a.lock().unwrap().is_empty());
    assert!(b.lock().unwrap().is_empty());
    assert!(bad.lock().unwrap().is_empty());
    send(first, "/missing", 404).await;
    assert!(a.lock().unwrap().is_empty());
    assert!(send(first, "/deferred", 200).await.contains('3'));
    assert!(send(second, "/deferred", 200).await.contains('9'));
    assert!(send(first, "/deferred", 200).await.contains('3'));
    for _ in 0..2 {
        let body = send(failed, "/deferred", 500).await;
        assert!(!body.contains("private boot detail"));
    }
    assert_eq!(a.lock().unwrap().len(), 3);
    assert_eq!(b.lock().unwrap().len(), 3);
    assert_eq!(bad.lock().unwrap().len(), 2);
    for task in [x, y, z] {
        task.abort();
        let _ = task.await;
    }
}

struct CycleValue<const N: u8>;
struct CycleProvider<const N: u8>(Arc<tokio::sync::Barrier>);
impl<const N: u8> ServiceProvider for CycleProvider<N> {
    #[allow(non_snake_case)]
    fn isDeferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<CycleValue<N>>()]
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.0.wait().await;
            if N == 1 {
                app.make::<CycleValue<2>>().await?;
            } else {
                app.make::<CycleValue<1>>().await?;
            }
            app.container().instance(Arc::new(CycleValue::<N>))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn independently_polled_provider_boots_detect_wait_cycles_instead_of_deadlocking() {
    let root = Root::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(CycleProvider::<1>(barrier.clone())),
            Box::new(CycleProvider::<2>(barrier)),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let mut one = Box::pin(app.make::<CycleValue<1>>());
    let mut two = Box::pin(app.make::<CycleValue<2>>());
    std::future::poll_fn(|cx| {
        assert!(one.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    // Each caller starts in its own task-local lineage, as separate HTTP requests do.
    let (a, b) = tokio::join!(one.as_mut(), two.as_mut());
    assert!(a.is_err());
    assert!(b.is_err());
}

struct Retrying {
    constructions: Arc<AtomicUsize>,
    ordinal: usize,
}
impl openrust::contracts::support::DeferrableProvider for Retrying {
    fn provides() -> Vec<ServiceKey> {
        vec!["retry.first".into(), "retry.second".into()]
    }
}
impl ServiceProvider for Retrying {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert_eq!(self.constructions.load(Ordering::SeqCst), self.ordinal);
            app.container().instance(Arc::new(self.ordinal as u16))?;
            Err(std::io::Error::other("registration failed after contribution").into())
        })
    }
}
#[tokio::test]
async fn compiled_factories_are_lazy_and_retry_fresh_instances_only_for_unconsumed_keys() {
    let root = Root::new();
    let constructions = Arc::new(AtomicUsize::new(0));
    let observed = constructions.clone();
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(openrust::support::DeferredProvider::new(
            move || {
                let ordinal = observed.fetch_add(1, Ordering::SeqCst) + 1;
                Retrying {
                    constructions: observed.clone(),
                    ordinal,
                }
            },
        ))])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    for (key, count) in [
        ("retry.first", 1),
        ("retry.first", 1),
        ("retry.second", 2),
        ("retry.second", 2),
    ] {
        assert!(app.get::<u16>(key).await.is_err());
        assert!(!app.isDeferredService(key));
        assert_eq!(constructions.load(Ordering::SeqCst), count);
        assert_eq!(*app.make::<u16>().await.unwrap(), count as u16);
    }
}

struct Multiple;
impl openrust::contracts::support::DeferrableProvider for Multiple {
    fn provides() -> Vec<ServiceKey> {
        vec!["multiple.first".into(), "multiple.second".into()]
    }
}
impl ServiceProvider for Multiple {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .instanceNamed("multiple.first", Arc::new(1_u8))?;
            app.container()
                .instanceNamed("multiple.second", Arc::new(2_u8))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn loading_one_key_retains_other_manifest_keys_until_explicit_load_all() {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(openrust::support::DeferredProvider::new(
            || Multiple,
        ))])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(*app.get::<u8>("multiple.first").await.unwrap(), 1);
    assert!(!app.isDeferredService("multiple.first"));
    assert!(app.isDeferredService("multiple.second"));
    assert_eq!(*app.get::<u8>("multiple.second").await.unwrap(), 2);
    assert!(app.isDeferredService("multiple.second"));
    app.loadDeferredProviders().await.unwrap();
    assert!(!app.isDeferredService("multiple.second"));
}

struct EagerTrigger {
    trace: Arc<Mutex<Vec<&'static str>>>,
    during_register: bool,
}
impl ServiceProvider for EagerTrigger {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("eager.register.begin");
            if self.during_register {
                app.make::<Service>().await?;
            }
            self.trace.lock().unwrap().push("eager.register.end");
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.trace.lock().unwrap().push("eager.boot.begin");
            if !self.during_register {
                app.make::<Service>().await?;
            }
            self.trace.lock().unwrap().push("eager.boot.end");
            Ok(())
        })
    }
}
#[tokio::test]
async fn deferred_first_use_during_register_and_boot_preserves_laravel_callback_order() {
    let root = Root::new();
    for during_register in [true, false] {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let mut app = Application::configure(root.0.clone())
            .withConfig(config)
            .withProviders(vec![
                Box::new(Lazy {
                    value: 4,
                    trace: trace.clone(),
                    pause: None,
                    fail: false,
                }),
                Box::new(EagerTrigger {
                    trace: trace.clone(),
                    during_register,
                }),
            ])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let actual = trace.lock().unwrap().clone();
        if during_register {
            assert_eq!(
                actual,
                [
                    "eager.register.begin",
                    "lazy.register",
                    "eager.register.end",
                    "lazy.boot.begin",
                    "lazy.boot.end",
                    "eager.boot.begin",
                    "eager.boot.end",
                    "lazy.boot.begin",
                    "lazy.boot.end"
                ]
            );
        } else {
            assert_eq!(
                actual,
                [
                    "eager.register.begin",
                    "eager.register.end",
                    "eager.boot.begin",
                    "lazy.register",
                    "eager.boot.end",
                    "lazy.boot.begin",
                    "lazy.boot.end"
                ]
            );
        }
    }
}
#[tokio::test]
async fn pending_deferred_contracts_reject_conditional_overwrites_without_constructing_them() {
    let root = Root::new();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut app = application(&root, 3, trace, None, false);
    app.container()
        .bindIf::<Service, _>(|_| panic!("conditional transient replaced a deferred contract"))
        .unwrap();
    app.container()
        .singletonIf::<Service, _>(|_| panic!("conditional singleton replaced a deferred contract"))
        .unwrap();
    app.container()
        .scopedIf::<Service, _>(|_| panic!("conditional scoped replaced a deferred contract"))
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(app.make::<Service>().await.unwrap().0, 3);
}

struct Control(Arc<(Notify, Notify)>);
struct Controls;
impl Facade for Controls {
    type Service = Control;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Control>()
    }
}
fn concurrent_facade_read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let control = Controls::getFacadeRoot().unwrap();
        let mut first = Box::pin(ServiceFacade::getFacadeRootAsync());
        tokio::select! {value=&mut first=>panic!("early facade service {}",value.unwrap().0),_=control.0.0.notified()=>{}}
        ServiceFacade::swap(Arc::new(Service(99))).unwrap();
        assert!(matches!(
            ServiceFacade::getFacadeRoot(),
            Err(BindingResolutionError::DeferredServiceRequiresAsync { .. })
        ));
        let mut second = Box::pin(ServiceFacade::getFacadeRootAsync());
        std::future::poll_fn(|cx| {
            assert!(second.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        control.0.1.notify_one();
        let (a, b) = tokio::join!(first, second);
        assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
        Ok(Response::new(StatusCode::OK, Body::from("ready")))
    })
}
#[tokio::test]
async fn boot_owner_facade_cache_cannot_leak_readiness_to_another_future_in_the_same_request() {
    let root = Root::new();
    let pause = Arc::new((Notify::new(), Notify::new()));
    let app = application(
        &root,
        7,
        Arc::new(Mutex::new(Vec::new())),
        Some(pause.clone()),
        false,
    );
    app.container().instance(Arc::new(Control(pause))).unwrap();
    let (address, task) = server(app).await;
    assert!(send(address, "/concurrent", 200).await.contains("ready"));
    task.abort();
    let _ = task.await;
}

fn invalid_middleware_routes(_: &Application) -> Result<openrust::routing::Router, RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.group(
        openrust::routing::RouteGroup::new().middleware_name("undefined"),
        |router| router.get("/", read).map(|_| ()),
    )?;
    Ok(router)
}
#[tokio::test]
async fn a_middleware_bootstrap_error_remains_reportable_on_repeated_kernel_bootstrap() {
    let root = Root::new();
    let app = Application::configure(root.0.clone())
        .withConfig(config)
        .withRouting(Routing::new(invalid_middleware_routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(app);
    assert!(kernel.bootstrap().await.is_err());
    assert!(kernel.bootstrap().await.is_err());
}

struct Panicking;
impl openrust::contracts::support::DeferrableProvider for Panicking {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<u64>()]
    }
}
impl ServiceProvider for Panicking {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().instance(Arc::new(1_u64))?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, _: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async { panic!("boot panic") })
    }
}
#[tokio::test]
async fn a_panicking_boot_releases_waiters_without_publishing_its_partial_service() {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(openrust::support::DeferredProvider::new(
            || Panicking,
        ))])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let app = Arc::new(app);
    let task_app = app.clone();
    let task = tokio::spawn(async move { task_app.make::<u64>().await });
    assert!(task.await.unwrap_err().is_panic());
    assert!(matches!(
        app.make::<u64>().await,
        Err(BindingResolutionError::DeferredProviderFailed { .. })
    ));
    assert!(app.container().make::<u64>().is_err());
}
#[tokio::test]
async fn deferred_factory_type_deduplication_keeps_the_first_registration() {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(openrust::support::DeferredProvider::new(|| Multiple)),
            Box::new(openrust::support::DeferredProvider::new(|| -> Multiple {
                panic!("duplicate factory invoked")
            })),
        ])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(*app.get::<u8>("multiple.first").await.unwrap(), 1);
}
