use openrust::{
    config::{ConfigError, Environment, Value},
    container::{ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
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

fn parameters(value: u32) -> ParameterOverrides {
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("number"), ParameterValue::new(value));
    args
}
struct Report(String);
struct Reports;
impl Facade for Reports {
    type Service = Report;
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<Report>()
    }
}
struct BadReports;
impl Facade for BadReports {
    type Service = Report;
    fn getFacadeAccessor() -> ServiceKey {
        "bad-report".into()
    }
}
struct Provider {
    name: &'static str,
}
impl DeferrableProvider for Provider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Report>(), "bad-report".into()]
    }
}
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .singletonAsync::<u32, _>(|c| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        let value = c
                            .getLastParameterOverride()
                            .get(&ParameterKey::name("number"))
                            .map(|v| v.downcast::<u32>())
                            .transpose()?
                            .map_or(0, |v| *v);
                        Ok(Arc::new(value))
                    })
                })
                .await?;
            app.container()
                .bindAsyncNamed::<Report, _>("bad-report", |c| {
                    Box::pin(async move {
                        let mut args = parameters(0);
                        args.insert(
                            ParameterKey::name("number"),
                            ParameterValue::new("private request argument"),
                        );
                        let value = c.makeWithAsync::<u32>(args).await?;
                        Ok(Arc::new(Report(value.to_string())))
                    })
                })
                .await?;
            let name = self.name;
            app.container()
                .bindAsync::<Report, _>(move |c| {
                    Box::pin(async move {
                        let overridden = c.makeWithAsync::<u32>(parameters(7)).await?;
                        let normal = c.makeAsync::<u32>().await?;
                        let failed = {
                            let mut args = parameters(0);
                            args.insert(
                                ParameterKey::name("number"),
                                ParameterValue::new("private bad value"),
                            );
                            c.makeWithAsync::<u32>(args).await
                        };
                        assert!(matches!(
                            failed,
                            Err(BindingResolutionError::ParameterTypeMismatch { .. })
                        ));
                        let retry = c.makeWithAsync::<u32>(parameters(9)).await?;
                        assert!(Arc::ptr_eq(&normal, &c.makeAsync::<u32>().await?));
                        Ok(Arc::new(Report(format!(
                            "{name}:{overridden}:{normal}:{retry}"
                        ))))
                    })
                })
                .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert_eq!(*app.makeWith::<u32>(parameters(6)).await?, 6);
            assert!(!app.container().resolved(ServiceKey::typed::<u32>())?);
            Ok(())
        })
    }
}
fn read(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let invalid = request.uri().path() == "/invalid";
    Box::pin(async move {
        let report = if invalid {
            BadReports::getFacadeRootAsync().await
        } else {
            Reports::getFacadeRootAsync().await
        }
        .map_err(|source| HttpError::Internal {
            source: source.into(),
        })?;
        Ok(Response::new(StatusCode::OK, Body::from(report.0.clone())))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/parameters", read)?;
    Route::get("/invalid", read)
}
fn application(root: &Root, name: &'static str) -> Application {
    Application::configure(root.0.clone())
        .withConfig(config)
        .withProviders(vec![Box::new(DeferredProvider::new(move || Provider {
            name,
        }))])
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
async fn http_deferred_providers_apply_parameters_without_polluting_shared_caches_or_other_apps() {
    let root = Root::new();
    let (a, x) = server(application(&root, "a")).await;
    let (b, y) = server(application(&root, "b")).await;
    assert_eq!(send(a, "/parameters", 200).await, "a:7:0:9");
    assert_eq!(send(b, "/parameters", 200).await, "b:7:0:9");
    let failure = send(a, "/invalid", 500).await;
    assert!(!failure.contains("private request argument"));
    assert_eq!(send(a, "/parameters", 200).await, "a:7:0:9");
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}
