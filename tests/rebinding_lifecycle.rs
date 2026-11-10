use openrust::{
    config::{ConfigError, Environment, Repository, Value},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::ServiceKey,
    contracts::{
        config::Repository as RepositoryContract, console::Kernel as _, container::Container as _,
        foundation::Application as _, http::Kernel as _,
    },
    foundation::{Application, BootstrapError, configuration::Routing, console, http::Kernel},
    http::Request,
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Facade, Route},
    },
    view::view,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-rebinding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(path.join("public")).unwrap();
        std::fs::create_dir_all(path.join("themes/provider")).unwrap();
        std::fs::create_dir_all(path.join("storage/app/public")).unwrap();
        std::fs::write(path.join("themes/provider/welcome.html"), "provider view").unwrap();
        std::fs::write(
            path.join("storage/app/public/probe.txt"),
            "provider storage",
        )
        .unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn repository(name: &str) -> Arc<dyn RepositoryContract> {
    Arc::new(Repository::new(BTreeMap::from([
        (
            "app".into(),
            json!({"name":name,"host":"127.0.0.1","port":0,"url":"http://localhost"}),
        ),
        ("view".into(), json!({"paths":["themes/provider"]})),
        (
            "filesystems".into(),
            json!({"disks":{"public":{"driver":"local","root":"storage/app/public","visibility":"public","url":"http://localhost/storage/tmp/"}}}),
        ),
    ])))
}
fn seed(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"name":"seed","host":"127.0.0.1","port":70000}),
        ),
        (
            "filesystems".into(),
            json!({"disks":{"public":{"driver":"local","root":"wrong","url":"http://localhost/wrong"}}}),
        ),
    ]))
}
struct Provider {
    repository: Arc<dyn RepositoryContract>,
    seen: Arc<Mutex<Vec<String>>>,
}
impl ServiceProvider for Provider {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let previous = app
                .container()
                .refresh::<dyn RepositoryContract, _, _>(
                    "config",
                    self.seen.clone(),
                    |seen, config| {
                        seen.lock()
                            .unwrap()
                            .push(config.get("app.name").unwrap().as_str().unwrap().to_owned());
                        Ok(())
                    },
                )?
                .unwrap();
            assert_eq!(previous.get("app.name"), Some(json!("seed")));
            app.container()
                .instanceNamed("config", self.repository.clone())?;
            assert!(Arc::ptr_eq(&app.config()?, &self.repository));
            assert!(Arc::ptr_eq(
                &app.config()?,
                &app.container().make::<dyn RepositoryContract>()?
            ));
            Ok(())
        })
    }
}
fn web() -> Result<(), RoutingError> {
    Route::get("/", || view("welcome"))?;
    Route::staticPublic()?;
    Route::staticDisk("public")
}
struct ConfigCommand(CommandDefinition);
impl Command for ConfigCommand {
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
            let current = app
                .config()
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            output
                .stdout
                .push_str(current.get("app.name").unwrap().as_str().unwrap());
            Ok(0)
        })
    }
}
struct Swapped;
impl Facade for Swapped {
    type Service = u32;
    fn getFacadeAccessor() -> ServiceKey {
        "swap-probe".into()
    }
}
struct SwapCommand(CommandDefinition);
impl Command for SwapCommand {
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
            app.container()
                .instanceNamed("swap-probe", Arc::new(1_u32))
                .unwrap();
            assert_eq!(*Swapped::getFacadeRoot().unwrap(), 1);
            app.container()
                .rebinding::<u32, _>("swap-probe", |_, value| {
                    assert!(Arc::ptr_eq(&value, &Swapped::getFacadeRoot()?));
                    Err(
                        openrust::contracts::container::BindingResolutionError::NotBound {
                            service: "swap-listener".into(),
                        },
                    )
                })
                .unwrap();
            assert!(Swapped::swap(Arc::new(2)).is_err());
            assert_eq!(*Swapped::getFacadeRoot().unwrap(), 2);
            assert_eq!(*app.container().get::<u32>("swap-probe").unwrap(), 2);
            output.stdout.push_str("swapped");
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ConfigCommand(CommandDefinition::new(
        clap::Command::new("config:probe"),
    ))))?;
    artisan.add_command(Box::new(SwapCommand(CommandDefinition::new(
        clap::Command::new("swap:probe"),
    ))))
}
fn application(root: &Root, name: &str, seen: Arc<Mutex<Vec<String>>>) -> Application {
    Application::configure(root.0.clone())
        .withConfig(seed)
        .withProviders(vec![Box::new(Provider {
            repository: repository(name),
            seen,
        })])
        .withCommands(commands)
        .withExceptions(|exceptions| {
            exceptions.shouldRenderJsonWhen(|_, _| true);
        })
        .withRouting(Routing::default().web(web).health("/up"))
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
async fn server(
    app: Application,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
    Arc<Kernel>,
) {
    use axum::handler::Handler as _;
    let mut kernel = Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let service = transport.with_state(kernel.clone()).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
        kernel,
    )
}
fn send(address: std::net::SocketAddr, path: &str, status: u16) -> String {
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream.set_write_timeout(Some(timeout)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: text/html\r\nConnection: close\r\n\r\n"
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
async fn current_config_drives_the_listener_routes_views_health_and_two_application_isolation() {
    let root = Root::new();
    let seen_a = Arc::new(Mutex::new(Vec::new()));
    let seen_b = Arc::new(Mutex::new(Vec::new()));
    let ((a, x, first), (b, y, second)) = tokio::join!(
        server(application(&root, "A:provider", seen_a.clone())),
        server(application(&root, "B:provider", seen_b.clone()))
    );
    assert_eq!(*seen_a.lock().unwrap(), ["A:provider"]);
    assert_eq!(*seen_b.lock().unwrap(), ["B:provider"]);
    let bodies = tokio::task::spawn_blocking(move || {
        [
            send(a, "/", 200),
            send(a, "/storage/tmp/probe.txt", 200),
            send(a, "/up", 200),
            send(b, "/up", 200),
            send(a, "/wrong/probe.txt", 404),
        ]
    })
    .await
    .unwrap();
    assert_eq!(bodies[0], "provider view");
    assert_eq!(bodies[1], "provider storage");
    assert!(bodies[2].contains("A:provider"));
    assert!(bodies[3].contains("B:provider"));
    let old = first.get_application().config().unwrap();
    let current = repository("A:runtime");
    first
        .get_application()
        .container()
        .instanceNamed("config", current.clone())
        .unwrap();
    assert!(Arc::ptr_eq(
        &first.get_application().config().unwrap(),
        &current
    ));
    assert_eq!(old.get("app.name"), Some(json!("A:provider")));
    let bodies = tokio::task::spawn_blocking(move || [send(a, "/up", 200), send(b, "/up", 200)])
        .await
        .unwrap();
    assert!(bodies[0].contains("A:runtime"));
    assert!(!bodies[0].contains("A:provider"));
    assert!(bodies[1].contains("B:provider"));
    assert!(
        first
            .get_application()
            .container()
            .instanceNamed("config", Arc::new(String::from("private value")))
            .is_err()
    );
    assert!(first.get_application().config().is_err());
    let failed = tokio::task::spawn_blocking(move || send(a, "/up", 500))
        .await
        .unwrap();
    assert!(failed.contains("Internal Server Error"));
    assert!(!failed.contains("private value"));
    first
        .get_application()
        .container()
        .instanceNamed("config", current)
        .unwrap();
    assert!(
        tokio::task::spawn_blocking(move || send(a, "/up", 200))
            .await
            .unwrap()
            .contains("A:runtime")
    );
    assert_eq!(
        second.get_application().config().unwrap().get("app.name"),
        Some(json!("B:provider"))
    );
    x.abort();
    y.abort();
}
#[tokio::test]
async fn artisan_uses_rebound_configuration_and_preserves_facade_swap_commit_order() {
    let root = Root::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut kernel = console::Kernel::new(
        application(&root, "CLI:provider", seen.clone()),
        openrust::console::Application::new(),
    );
    for (name, expected) in [(None, "CLI:provider"), (Some("CLI:runtime"), "CLI:runtime")] {
        if let Some(name) = name {
            kernel
                .get_application()
                .container()
                .instanceNamed("config", repository(name))
                .unwrap();
        }
        let mut output = Output::default();
        assert_eq!(
            kernel.handle(&["config:probe".into()], &mut output).await,
            0
        );
        assert_eq!(output.stdout, expected);
    }
    assert!(
        kernel
            .get_application()
            .container()
            .instanceNamed("config", Arc::new(1_u32))
            .is_err()
    );
    let mut output = Output::default();
    assert_eq!(
        kernel.handle(&["config:probe".into()], &mut output).await,
        1
    );
    assert!(!output.stderr.is_empty());
    kernel
        .get_application()
        .container()
        .instanceNamed("config", repository("CLI:restored"))
        .unwrap();
    let mut output = Output::default();
    assert_eq!(
        kernel.handle(&["config:probe".into()], &mut output).await,
        0
    );
    assert_eq!(output.stdout, "CLI:restored");
    let mut output = Output::default();
    assert_eq!(kernel.handle(&["swap:probe".into()], &mut output).await, 0);
    assert_eq!(output.stdout, "swapped");
    assert_eq!(
        *seen.lock().unwrap(),
        ["CLI:provider", "CLI:runtime", "CLI:restored"]
    );
}
#[tokio::test]
async fn a_configuration_listener_failure_stops_boot_before_loading_files_and_keeps_the_new_repository()
 {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(|_, _| panic!("configuration must not load after a rebound failure"))
        .create()
        .unwrap();
    app.container()
        .instanceNamed("config", repository("preboot"))
        .unwrap();
    let initial = app.config().unwrap();
    app.container()
        .rebinding::<dyn RepositoryContract, _>("config", |_, _| {
            Err(
                openrust::contracts::container::BindingResolutionError::NotBound {
                    service: "boot-listener".into(),
                },
            )
        })
        .unwrap();
    assert!(matches!(
        app.boot().await,
        Err(BootstrapError::ContainerFailed { .. })
    ));
    assert!(!Arc::ptr_eq(&initial, &app.config().unwrap()));
    assert!(app.config().unwrap().all().is_empty());
    assert!(matches!(
        app.boot().await,
        Err(BootstrapError::PreviousBootFailed)
    ));
}

#[tokio::test]
async fn configuration_is_unbound_before_boot_and_the_new_observed_repository_is_filled_in_place() {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .withConfig(seed)
        .create()
        .unwrap();
    assert!(app.config().is_err());
    assert_eq!(
        app.container()
            .getAlias(ServiceKey::typed::<dyn RepositoryContract>())
            .unwrap(),
        ServiceKey::named("config")
    );
    let initial = repository("preboot");
    app.container()
        .instanceNamed("config", initial.clone())
        .unwrap();
    let observed: Arc<Mutex<Option<Arc<dyn RepositoryContract>>>> = Arc::new(Mutex::new(None));
    let seen = observed.clone();
    app.container()
        .rebinding::<dyn RepositoryContract, _>("config", move |_, value| {
            assert!(value.all().is_empty());
            *seen.lock().unwrap() = Some(value);
            Ok(())
        })
        .unwrap();
    assert!(observed.lock().unwrap().is_none());
    app.boot().await.unwrap();
    let current = app.config().unwrap();
    assert!(Arc::ptr_eq(
        observed.lock().unwrap().as_ref().unwrap(),
        &current
    ));
    assert_eq!(current.get("app.name"), Some(json!("seed")));
    assert_eq!(initial.get("app.name"), Some(json!("preboot")));
}
