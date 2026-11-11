use openrust::{
    config::{ConfigError, Environment, Value},
    container::{
        Bind, ClassDescriptor, Container, EnvironmentQuery, EnvironmentResult, ServiceKey,
    },
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
trait Selected: Send + Sync {
    fn label(&self) -> &'static str;
}
struct Testing;
struct DefaultSelection;
impl Selected for Testing {
    fn label(&self) -> &'static str {
        "testing"
    }
}
impl Selected for DefaultSelection {
    fn label(&self) -> &'static str {
        "fallback"
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
            let container = app.container();
            container.registerClass(ClassDescriptor::concrete(vec![], |_| {
                Ok(Arc::new(Testing))
            })?)?;
            container.registerClass(ClassDescriptor::concrete(vec![], |_| {
                Ok(Arc::new(DefaultSelection))
            })?)?;
            container.registerBindingAttributes::<dyn Selected>(vec![
                Bind::new::<dyn Selected, DefaultSelection>(|value| value).into(),
                Bind::new::<dyn Selected, Testing>(|value| value)
                    .forEnvironments(["testing"])
                    .into(),
            ])?;
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
fn read_binding(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let selected = Container::getInstance()
            .makeAsync::<dyn Selected>()
            .await
            .map_err(internal)?;
        Ok(Response::new(StatusCode::OK, Body::from(selected.label())))
    })
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/environment", read)?;
    Route::get("/bad", bad)?;
    Route::get("/fix", fix)?;
    Route::get("/binding", read_binding)
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
    assert!(
        !send(first, "/binding", 500)
            .await
            .contains("private invalid environment")
    );
    assert_eq!(send(first, "/fix", 200).await, "testing:true");
    let (testing, production) =
        tokio::join!(send(first, "/binding", 200), send(second, "/binding", 200));
    assert_eq!(testing, "testing");
    assert_eq!(production, "fallback");
    assert_eq!(send(second, "/environment", 200).await, "production:false");
    assert_eq!(send(first, "/environment", 200).await, "testing:true");
    for task in [x, y] {
        task.abort();
        let _ = task.await;
    }
}

struct DefaultEnvironmentProvider;

impl ServiceProvider for DefaultEnvironmentProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let name = app.environment(None)?;
            let EnvironmentResult::String(current) = &name else {
                panic!("a query without patterns must return the name");
            };
            assert_eq!(app.environment(Some(current.as_ref().into()))?, true.into());
            assert_eq!(app.environment(Some("QA.*".into()))?, false.into());
            assert_eq!(app.environment(Some((&[]).into()))?, false.into());
            assert_eq!(app.container().currentEnvironmentIs(&[])?, false.into());
            app.config()?
                .set("app.env", Value::String("changed".into()))?;
            assert_eq!(app.environment(None)?, name);

            app.container().bind::<Report, _>(|container| {
                let name = container.get::<str>("env")?;
                let matches = bool::from(container.currentEnvironmentIs(&["testing", "qa.*"])?);
                Ok(Arc::new(Report(format!("{name}:{matches}"))))
            })?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn configured_environment_is_available_to_providers_and_isolated_across_http_apps() {
    let root = Root::new();
    let builder = || {
        Application::configure(root.0.clone())
            .withProviders(vec![Box::new(DefaultEnvironmentProvider)])
            .withRouting(Routing::default().web(routes))
    };
    let configured = builder()
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"host":"127.0.0.1","port":0,"env":"qa.部署"}),
            )]))
        })
        .create()
        .unwrap();
    let fallback = builder().withConfig(config).create().unwrap();
    let invalid = builder()
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                serde_json::json!({"env": false}),
            )]))
        })
        .create()
        .unwrap();
    let error = Kernel::new(invalid).listen().await.unwrap_err();
    assert!(error.to_string().contains("app.env must be a string"));
    assert!(configured.environment(None).is_err());

    let (first, a) = server(configured).await;
    let (second, b) = server(fallback).await;
    let (first_response, second_response) = tokio::join!(
        send(first, "/environment", 200),
        send(second, "/environment", 200),
    );
    assert_eq!(first_response, "qa.部署:true");
    assert_eq!(second_response, "production:false");
    for task in [a, b] {
        task.abort();
        let _ = task.await;
    }
}

#[test]
fn artisan_environment_option_selects_files_and_overrides_the_detected_name() {
    use std::{fs, process::Command};

    let root = Root::new();
    let run = |arguments: &[&str], process_environment: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .env_clear()
            .env("APP_BASE_PATH", &root.0)
            .arg("artisan")
            .args(arguments);
        if let Some(environment) = process_environment {
            command.env("APP_ENV", environment);
        }
        command.output().unwrap()
    };
    let expect = |arguments: &[&str], process_environment, name: &str| {
        let output = run(arguments, process_environment);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("\n   INFO  The application environment is [{name}].\n\n")
        );
    };
    fs::write(root.0.join(".env"), "APP_ENV=from-dotenv\n").unwrap();
    fs::write(root.0.join(".env.from-dotenv"), "APP_NAME=\"bad\n").unwrap();
    expect(&["env"], None, "from-dotenv");
    fs::write(root.0.join(".env.local"), "APP_ENV=inside-file\n").unwrap();
    expect(&["--env", "local", "env"], None, "local");
    expect(&["env", "--env=local"], Some("external"), "local");
    expect(&["env", "--env=missing"], None, "missing");
    expect(&["--env=local", "env", "--env=second"], None, "local");

    fs::write(root.0.join(".env.external"), "APP_NAME=\"bad\n").unwrap();
    expect(&["env", "--env=local"], Some("external"), "local");
    let invalid = run(&["env", "--env=missing"], Some("external"));
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains(".env.external"));
    fs::write(root.0.join(".env.external"), "APP_ENV=file\n").unwrap();
    expect(&["env"], Some("external"), "external");
    expect(&["env", "--env=missing"], Some("external"), "missing");

    fs::write(root.0.join(".env.local"), "APP_NAME=\"bad\n").unwrap();
    let invalid = run(&["env", "--env=local"], Some("external"));
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains(".env.local"));

    for arguments in [
        ["env", "--env"],
        ["env", "--env="],
        ["env", "--env=../secret"],
    ] {
        let output = run(&arguments, None);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("../secret"));
    }
}
