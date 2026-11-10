use openrust::{
    config::{ConfigError, Environment, Value},
    container::{ClassDescriptor, ConstructorParam, ParameterValue, ServiceKey},
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
    clients: AtomicUsize,
    workers: AtomicUsize,
}
struct Client {
    name: &'static str,
    attempt: usize,
}
struct Worker {
    client: Arc<Client>,
    sequence: usize,
    number: u32,
}
struct Workers;
impl Facade for Workers {
    type Service = Worker;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Worker>()
    }
}
struct Constructors(Arc<Counts>);
impl ServiceProvider for Constructors {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let counts = self.0.clone();
            app.container()
                .registerClass(ClassDescriptor::concrete::<Worker, _>(
                    vec![
                        ConstructorParam::class::<Client>("client"),
                        ConstructorParam::scalar::<u32>("number")
                            .withDefault(|| Ok(ParameterValue::new(1u32))),
                    ],
                    move |args| {
                        Ok(Arc::new(Worker {
                            client: args.service(0)?,
                            sequence: counts.workers.fetch_add(1, Ordering::SeqCst) + 1,
                            number: *args.get::<u32>(1)?,
                        }))
                    },
                )?)?;
            assert!(!app.container().bound::<Worker>());
            Ok(())
        })
    }
}
struct ClientProvider {
    counts: Arc<Counts>,
    name: &'static str,
    fail: bool,
}
impl DeferrableProvider for ClientProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Client>()]
    }
}
impl ServiceProvider for ClientProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let (counts, name, fail) = (self.counts.clone(), self.name, self.fail);
            app.container()
                .singletonAsync::<Client, _>(move |_| {
                    let counts = counts.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        let attempt = counts.clients.fetch_add(1, Ordering::SeqCst) + 1;
                        if fail && attempt == 1 {
                            return Err(BindingResolutionError::FactoryFailed {
                                service: "client",
                                source: std::io::Error::other("private client constructor failure")
                                    .into(),
                            });
                        }
                        Ok(Arc::new(Client { name, attempt }))
                    })
                })
                .await?;
            Ok(())
        })
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let a = Workers::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        let b = Workers::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        assert!(Arc::ptr_eq(&a, &b));
        Ok(Response::new(
            StatusCode::OK,
            Body::from(format!(
                "{}:{}:{}:{}",
                a.client.name, a.client.attempt, a.sequence, a.number
            )),
        ))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/worker", read)
}
fn application(root: &Root, name: &'static str, counts: Arc<Counts>, fail: bool) -> Application {
    let owned = counts.clone();
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(Constructors(counts)),
            Box::new(DeferredProvider::new(move || ClientProvider {
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
async fn real_http_automatically_constructs_metadata_and_loads_deferred_dependencies_with_retry_and_isolation()
 {
    let root = Root::new();
    let a = Arc::new(Counts::default());
    let b = Arc::new(Counts::default());
    let failed = Arc::new(Counts::default());
    let (first, x) = server(application(&root, "a", a.clone(), false)).await;
    let (second, y) = server(application(&root, "b", b.clone(), false)).await;
    let (bad, z) = server(application(&root, "retry", failed.clone(), true)).await;
    for count in [&a, &b, &failed] {
        assert_eq!(count.clients.load(Ordering::SeqCst), 0);
        assert_eq!(count.workers.load(Ordering::SeqCst), 0);
    }
    assert_eq!(send(first, "/worker", 200).await, "a:1:1:1");
    assert_eq!(send(first, "/worker", 200).await, "a:1:2:1");
    assert_eq!(send(second, "/worker", 200).await, "b:1:1:1");
    assert!(
        !send(bad, "/worker", 500)
            .await
            .contains("private client constructor failure")
    );
    assert_eq!(send(bad, "/worker", 200).await, "retry:2:1:1");
    for task in [x, y, z] {
        task.abort();
        let _ = task.await;
    }
}
