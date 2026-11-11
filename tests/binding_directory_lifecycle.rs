use openrust::{
    config::{ConfigError, Environment, Value},
    container::{Container, FactoryHandle, ParameterOverrides, ServiceKey},
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        http::Kernel as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
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

struct Directory {
    name: &'static str,
    container: Container,
    original: FactoryHandle,
    cleaned: Arc<AtomicUsize>,
}
struct ScopeCleanup(Arc<AtomicUsize>);
impl Drop for ScopeCleanup {
    fn drop(&mut self) {
        assert!(
            Directories::getFacadeRoot().is_err(),
            "cached values must not inherit the flushing request Facade context"
        );
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Directories;
impl Facade for Directories {
    type Service = Directory;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Directory>()
    }
}
struct Provider {
    name: &'static str,
    fail: bool,
    calls: Arc<AtomicUsize>,
}
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = Container::default();
            let calls = self.calls.clone();
            let fail = self.fail;
            container
                .singletonAsync::<u32, _>(move |_| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        if calls.fetch_add(1, Ordering::SeqCst) == 0 && fail {
                            return Err(BindingResolutionError::FactoryFailed {
                                service: "directory",
                                source: std::io::Error::other("private raw failure").into(),
                            });
                        }
                        Ok(Arc::new(1))
                    })
                })
                .await?;
            let cleaned = Arc::new(AtomicUsize::new(0));
            let observed = cleaned.clone();
            container
                .scoped::<ScopeCleanup, _>(move |_| Ok(Arc::new(ScopeCleanup(observed.clone()))))?;
            let original = container
                .getBindings()
                .get(&ServiceKey::typed::<u32>())
                .unwrap()
                .concrete
                .clone();
            app.container().instance(Arc::new(Directory {
                name: self.name,
                container,
                original,
                cleaned,
            }))?;
            Ok(())
        })
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let directory = Directories::getFacadeRootAsync().await.map_err(internal)?;
        let raw = directory
            .original
            .callAsync::<u32>(&directory.container, ParameterOverrides::new())
            .await
            .map_err(internal)?;
        let normal = directory
            .container
            .makeAsync::<u32>()
            .await
            .map_err(internal)?;
        Ok(Response::new(
            StatusCode::OK,
            Body::from(format!("{}:{raw}:{normal}", directory.name)),
        ))
    })
}
fn internal(source: BindingResolutionError) -> HttpError {
    HttpError::Internal {
        source: source.into(),
    }
}
fn reset(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let directory = Directories::getFacadeRootAsync().await.map_err(internal)?;
        directory
            .container
            .scope(async {
                directory
                    .container
                    .make::<ScopeCleanup>()
                    .map_err(internal)?;
                directory.container.flush();
                Ok::<(), HttpError>(())
            })
            .await?;
        assert_eq!(directory.cleaned.load(Ordering::SeqCst), 1);
        assert!(directory.container.getBindings().is_empty());
        directory
            .container
            .singleton::<u32, _>(|_| Ok(Arc::new(42)))
            .map_err(internal)?;
        Ok(Response::new(StatusCode::OK, Body::from("reset")))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/directory", read)?;
    Route::get("/reset", reset)
}
fn application(
    root: &Root,
    name: &'static str,
    fail: bool,
    calls: Arc<AtomicUsize>,
) -> Application {
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(Provider { name, fail, calls })])
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
async fn raw_factories_and_flushed_service_directories_work_over_http_and_remain_application_isolated()
 {
    let root = Root::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));
    let (first, x) = server(application(&root, "a", true, calls.clone())).await;
    let (second, y) = server(application(&root, "b", false, other.clone())).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let failure = send(first, "/directory", 500).await;
    assert!(!failure.contains("private raw failure"));
    assert_eq!(send(first, "/directory", 200).await, "a:1:1");
    assert_eq!(send(second, "/directory", 200).await, "b:1:1");
    assert_eq!(send(first, "/reset", 200).await, "reset");
    assert_eq!(send(first, "/directory", 200).await, "a:1:42");
    assert_eq!(send(second, "/directory", 200).await, "b:1:1");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(other.load(Ordering::SeqCst), 3);
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}
