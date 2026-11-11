use openrust::{
    config::{ConfigError, Environment, Value},
    container::{
        Callback, ClassDescriptor, ConstructorParam, MethodCall, ParameterKey, ParameterOverrides,
        ParameterValue, ServiceKey,
    },
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        http::Kernel as _,
        support::DeferrableProvider,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
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
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
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
    factory: AtomicUsize,
    methods: AtomicUsize,
}
struct Client(String);
struct Clients;
impl Facade for Clients {
    type Service = Client;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Client>()
    }
}
struct Worker;
struct Report(String);
struct Reports;
impl Facade for Reports {
    type Service = Report;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Report>()
    }
}
struct Registration {
    counts: Arc<Counts>,
    bound: bool,
}
impl ServiceProvider for Registration {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let c = app.container();
            c.registerClass(ClassDescriptor::concrete::<Worker, _>(vec![], |_| {
                Ok(Arc::new(Worker))
            })?)?;
            let count = self.counts.clone();
            c.registerMethod::<Worker, String, _>(
                "run",
                vec![
                    ConstructorParam::class::<Client>("client"),
                    ConstructorParam::scalar::<u32>("number")
                        .withDefault(|| Ok(ParameterValue::new(7u32))),
                ],
                move |_, args, _| {
                    let count = count.clone();
                    Box::pin(async move {
                        count.methods.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        let facade = Clients::getFacadeRootAsync().await?;
                        Ok(format!(
                            "{}:{}:{}",
                            args.resolved.service::<Client>(0)?.0,
                            *args.resolved.get::<u32>(1)?,
                            facade.0
                        ))
                    })
                },
            )?;
            if self.bound {
                c.bindMethod::<Worker, String, _>("run", |_, _| {
                    Box::pin(async {
                        Ok(format!("bound:{}", Clients::getFacadeRootAsync().await?.0))
                    })
                })?;
            }
            let invalid = self.bound;
            c.bindAsync::<Report, _>(move |c| {
                Box::pin(async move {
                    let lazy = c.wrap(
                        Callback::reusable(&[], move |_| async move {
                            let receiver = c.factoryAsync::<Worker>()().await?;
                            let mut input = ParameterOverrides::new();
                            if invalid {
                                input.insert(ParameterKey::name("number"), ParameterValue::null());
                            }
                            c.call(MethodCall::on(receiver, "run"), input).await
                        }),
                        ParameterOverrides::new(),
                    );
                    Ok(Arc::new(Report(lazy().await?)))
                })
            })
            .await?;
            Ok(())
        })
    }
}
struct Deferred {
    counts: Arc<Counts>,
    name: &'static str,
    fail: bool,
}
impl DeferrableProvider for Deferred {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Client>()]
    }
}
impl ServiceProvider for Deferred {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let (counts, name, fail) = (self.counts.clone(), self.name, self.fail);
            app.container()
                .singletonAsync::<Client, _>(move |_| {
                    let counts = counts.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        if counts.factory.fetch_add(1, Ordering::SeqCst) == 0 && fail {
                            return Err(BindingResolutionError::FactoryFailed {
                                service: "client",
                                source: std::io::Error::other("private client failure").into(),
                            });
                        }
                        Ok(Arc::new(Client(name.into())))
                    })
                })
                .await?;
            Ok(())
        })
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let report = Reports::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        Ok(Response::new(StatusCode::OK, Body::from(report.0.clone())))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/method", read)
}
fn application(
    root: &Root,
    name: &'static str,
    counts: Arc<Counts>,
    fail: bool,
    bound: bool,
) -> Application {
    let owned = counts.clone();
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(Registration { counts, bound }),
            Box::new(DeferredProvider::new(move || Deferred {
                counts: owned.clone(),
                name,
                fail,
            })),
        ])
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|_, _| true);
        })
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap()
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
async fn methods_over_http_wait_for_deferred_services_retry_failures_and_isolate_application_bindings()
 {
    let root = Root::new();
    let a = Arc::new(Counts::default());
    let b = Arc::new(Counts::default());
    let (first, x) = server(application(&root, "a", a.clone(), true, false)).await;
    let (second, y) = server(application(&root, "b", b.clone(), false, true)).await;
    assert_eq!(a.factory.load(Ordering::SeqCst), 0);
    let failure = send(first, "/method", 500).await;
    assert!(!failure.contains("private client failure"));
    assert_eq!(send(first, "/method", 200).await, "a:7:a");
    assert_eq!(send(second, "/method", 200).await, "bound:b");
    assert_eq!(send(first, "/method", 200).await, "a:7:a");
    assert_eq!(send(second, "/method", 200).await, "bound:b");
    assert_eq!(a.factory.load(Ordering::SeqCst), 2);
    assert_eq!(b.factory.load(Ordering::SeqCst), 1);
    assert_eq!(a.methods.load(Ordering::SeqCst), 2);
    assert_eq!(b.methods.load(Ordering::SeqCst), 0);
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}

#[tokio::test]
async fn application_call_and_direct_binding_restore_each_apps_facades_across_awaits() {
    let root = Root::new();
    let mut a = application(&root, "a", Arc::new(Counts::default()), false, false);
    let mut b = application(&root, "b", Arc::new(Counts::default()), false, true);
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    let (x, y) = tokio::join!(
        a.call::<String, _>(
            MethodCall::<Worker>::class("run"),
            ParameterOverrides::new()
        ),
        b.call::<String, _>(
            MethodCall::<Worker>::class("run"),
            ParameterOverrides::new()
        )
    );
    assert_eq!(x.unwrap(), "a:7:a");
    assert_eq!(y.unwrap(), "bound:b");
    assert_eq!(
        b.callMethodBinding::<Worker, String>("run", Arc::new(Worker))
            .await
            .unwrap(),
        "bound:b"
    );
}

#[tokio::test]
async fn application_lazy_closures_restore_facades_and_use_live_application_services() {
    let root = Root::new();
    let counts = Arc::new(Counts::default());
    let mut a = application(&root, "a", counts.clone(), false, false);
    let mut b = application(&root, "b", Arc::new(Counts::default()), false, true);
    a.boot().await.unwrap();
    b.boot().await.unwrap();
    a.container()
        .alias(ServiceKey::typed::<Client>(), "client")
        .unwrap();
    let make_a = a.factoryNamed::<Client>("client".into());
    let make_b = b.factory::<Client>();
    let wrap_a = a.wrap(
        Callback::reusable(&[], |_| async {
            tokio::task::yield_now().await;
            Ok(Clients::getFacadeRootAsync().await?.0.clone())
        }),
        ParameterOverrides::new(),
    );
    let wrap_b = b.wrap(
        Callback::reusable(&[], |_| async {
            tokio::task::yield_now().await;
            Ok(Clients::getFacadeRootAsync().await?.0.clone())
        }),
        ParameterOverrides::new(),
    );
    assert_eq!(counts.factory.load(Ordering::SeqCst), 0);
    let (x, y, u, v) = tokio::join!(make_a(), make_b(), wrap_a(), wrap_b());
    assert_eq!(
        (
            x.unwrap().0.as_str(),
            y.unwrap().0.as_str(),
            u.unwrap().as_str(),
            v.unwrap().as_str()
        ),
        ("a", "b", "a", "b")
    );
    a.container()
        .instance(Arc::new(Client("new".into())))
        .unwrap();
    assert_eq!(make_a().await.unwrap().0, "new");
    assert_eq!(wrap_a().await.unwrap(), "new");
    assert_eq!(wrap_b().await.unwrap(), "b");
}
