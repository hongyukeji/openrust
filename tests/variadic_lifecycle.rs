use openrust::{
    config::{ConfigError, Environment, Value},
    container::{ClassDescriptor, ConstructorParam, ServiceKey},
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
    first: AtomicUsize,
    second: AtomicUsize,
    groups: AtomicUsize,
}
trait Filter: Send + Sync {
    fn name(&self) -> &str;
}
struct Named(String);
impl Filter for Named {
    fn name(&self) -> &str {
        &self.0
    }
}
struct Filters(Vec<Arc<dyn Filter>>);
struct Missing(Vec<Arc<dyn Filter>>);
struct Groups;
impl Facade for Groups {
    type Service = Filters;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Filters>()
    }
}
struct MissingGroups;
impl Facade for MissingGroups {
    type Service = Missing;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Missing>()
    }
}
struct Registration {
    name: &'static str,
    counts: Arc<Counts>,
}
impl ServiceProvider for Registration {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let (counts, name) = (self.counts.clone(), self.name);
            app.container()
                .bindNamed::<dyn Filter, _>("first", move |_| {
                    counts.first.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(Named(format!("{name}-1"))))
                })?;
            let counts = self.counts.clone();
            app.container()
                .registerClass(ClassDescriptor::concrete::<Filters, _>(
                    vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
                    move |args| {
                        counts.groups.fetch_add(1, Ordering::SeqCst);
                        Ok(Arc::new(Filters(args.services(0)?)))
                    },
                )?)?;
            app.container()
                .when::<Filters>()?
                .needs::<dyn Filter>()
                .giveServices(["first", "second"])?;
            app.container()
                .registerClass(ClassDescriptor::concrete::<Missing, _>(
                    vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
                    |args| Ok(Arc::new(Missing(args.services(0)?))),
                )?)?;
            app.container()
                .when::<Missing>()?
                .needs::<dyn Filter>()
                .giveServices(["missing"])?;
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
        vec!["second".into()]
    }
}
impl ServiceProvider for Deferred {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let (counts, name, fail) = (self.counts.clone(), self.name, self.fail);
            app.container()
                .singletonAsyncNamed::<dyn Filter, _>("second", move |_| {
                    let counts = counts.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        let attempt = counts.second.fetch_add(1, Ordering::SeqCst) + 1;
                        if fail && attempt == 1 {
                            return Err(BindingResolutionError::FactoryFailed {
                                service: "second",
                                source: std::io::Error::other("private filter failure").into(),
                            });
                        }
                        Ok(Arc::new(Named(format!("{name}-2"))) as Arc<dyn Filter>)
                    })
                })
                .await?;
            Ok(())
        })
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let a = Groups::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        let b = Groups::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        assert!(Arc::ptr_eq(&a, &b));
        Ok(Response::new(
            StatusCode::OK,
            Body::from(a.0.iter().map(|f| f.name()).collect::<Vec<_>>().join(",")),
        ))
    })
}
fn missing(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let group = MissingGroups::getFacadeRootAsync()
            .await
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        Ok(Response::new(
            StatusCode::OK,
            Body::from(format!("empty:{}", group.0.len())),
        ))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/filters", read)?;
    Route::get("/missing", missing)
}
fn application(root: &Root, name: &'static str, counts: Arc<Counts>, fail: bool) -> Application {
    let owned = counts.clone();
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![
            Box::new(Registration { counts, name }),
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
async fn http_variadic_arrays_load_deferred_dependencies_retry_business_errors_and_isolate_apps() {
    let root = Root::new();
    let a = Arc::new(Counts::default());
    let b = Arc::new(Counts::default());
    let (first, x) = server(application(&root, "a", a.clone(), true)).await;
    let (second, y) = server(application(&root, "b", b.clone(), false)).await;
    assert_eq!(a.first.load(Ordering::SeqCst), 0);
    assert_eq!(a.second.load(Ordering::SeqCst), 0);
    let failure = send(first, "/filters", 500).await;
    assert!(!failure.contains("private filter failure"));
    assert_eq!(send(first, "/filters", 200).await, "a-1,a-2");
    assert_eq!(send(second, "/filters", 200).await, "b-1,b-2");
    assert_eq!(send(first, "/filters", 200).await, "a-1,a-2");
    assert_eq!(send(first, "/missing", 200).await, "empty:0");
    assert_eq!(a.first.load(Ordering::SeqCst), 3);
    assert_eq!(a.second.load(Ordering::SeqCst), 2);
    assert_eq!(a.groups.load(Ordering::SeqCst), 2);
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}
