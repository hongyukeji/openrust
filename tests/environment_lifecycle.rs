use openrust::{
    config::{ConfigError, Environment, Value},
    container::{EnvironmentQuery, EnvironmentResult, ServiceKey},
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

struct Configs;
impl Facade for Configs {
    type Service = dyn openrust::contracts::config::Repository;
    fn getFacadeAccessor() -> ServiceKey {
        "config".into()
    }
}
struct Report(String);
struct Reports;
impl Facade for Reports {
    type Service = Report;
    const cached: bool = false;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Report>()
    }
}
struct Provider(&'static str);
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app
                .get::<dyn openrust::contracts::config::Repository>("config")
                .await?;
            config.set("app.env", Value::String(self.0.into()))?;
            app.container()
                .resolveEnvironmentUsing(Some(Arc::new(move |query| {
                    let Some(Value::String(current)) = config.get("app.env") else {
                        return Err(BindingResolutionError::FactoryFailed {
                            service: "environment",
                            source: std::io::Error::other("private invalid environment").into(),
                        });
                    };
                    Ok(match query {
                        EnvironmentQuery::Name(name) => (name == current).into(),
                        EnvironmentQuery::Names([]) => current.into(),
                        EnvironmentQuery::Names(names) => names.contains(&current.as_str()).into(),
                    })
                })));
            app.container().bind::<Report, _>(|c| {
                let EnvironmentResult::String(current) = c.currentEnvironmentIs(&[])? else {
                    panic!("custom resolver must return string for empty array");
                };
                let matches = bool::from(c.currentEnvironmentIs("testing")?);
                Ok(Arc::new(Report(format!("{current}:{matches}"))))
            })?;
            Ok(())
        })
    }
}
fn internal(source: BindingResolutionError) -> HttpError {
    HttpError::Internal {
        source: source.into(),
    }
}
fn read(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let report = Reports::getFacadeRootAsync().await.map_err(internal)?;
        Ok(Response::new(StatusCode::OK, Body::from(report.0.clone())))
    })
}
fn bad(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Configs::getFacadeRoot()
            .map_err(internal)?
            .set("app.env", Value::Bool(false))
            .unwrap();
        read(request).await
    })
}
fn fix(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Configs::getFacadeRoot()
            .map_err(internal)?
            .set("app.env", Value::String("testing".into()))
            .unwrap();
        read(request).await
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/environment", read)?;
    Route::get("/bad", bad)?;
    Route::get("/fix", fix)
}
fn application(root: &Root, name: &'static str) -> Application {
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(Provider(name))])
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
async fn explicit_environment_resolvers_work_over_http_propagate_failures_retry_and_isolate_apps() {
    let root = Root::new();
    let (first, x) = server(application(&root, "local")).await;
    let (second, y) = server(application(&root, "production")).await;
    assert_eq!(send(first, "/environment", 200).await, "local:false");
    assert_eq!(send(second, "/environment", 200).await, "production:false");
    let failure = send(first, "/bad", 500).await;
    assert!(!failure.contains("private invalid environment"));
    assert_eq!(send(first, "/fix", 200).await, "testing:true");
    assert_eq!(send(second, "/environment", 200).await, "production:false");
    assert_eq!(send(first, "/environment", 200).await, "testing:true");
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}
