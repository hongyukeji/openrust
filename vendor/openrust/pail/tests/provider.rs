use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};

use openrust::{
    config::{ConfigError, Environment, Value},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing, http::Kernel, run_application},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    routing::RoutingError,
    support::facades::{Log, Route},
};
use openrust_pail::{File, Files, Handler, PailServiceProvider};
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

type SetupError = Box<dyn Error + Send + Sync>;
struct Root(PathBuf);
impl Root {
    fn new(environment: &str, pail: Option<&str>, vapor: Option<&str>) -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-pail-provider-{:032x}",
            fastrand::u128(..)
        ));
        std::fs::create_dir(&path).unwrap();
        let mut file = format!("APP_ENV={environment}\n");
        for (name, value) in [("PAIL_TESTS", pail), ("VAPOR_SSM_PATH", vapor)] {
            if let Some(value) = value {
                file.push_str(&format!(
                    "{name}={}\n",
                    serde_json::to_string(value).unwrap()
                ));
            }
        }
        std::fs::write(path.join(".env"), file).unwrap();
        File::new(path.join("storage/pail/session.pail"))
            .create()
            .unwrap();
        Self(path)
    }
    fn records(&self) -> Vec<Value> {
        std::fs::read_to_string(self.0.join("storage/pail/session.pail"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn config(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":env.get_utf8("APP_ENV")?.unwrap_or("production"),"host":"127.0.0.1","port":0,"observed_storage":openrust::support::storage_path("").unwrap()}),
        ),
        (
            "logging".into(),
            json!({"default":"single","channels":{"single":{"driver":"single","path":"original.log"}}}),
        ),
    ]))
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/probe", probe)
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Log::info("provider HTTP probe", &LogContext::new()).unwrap();
        Ok(Response::new(
            StatusCode::OK,
            Body::from(
                openrust::support::storage_path("")
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        ))
    })
}
struct Emit(CommandDefinition);
impl Command for Emit {
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
            Log::info("provider Artisan probe", &LogContext::new()).unwrap();
            let snapshot = app.container().make::<Environment>().unwrap();
            let file = app.container().get::<PathBuf>("env.file").unwrap();
            let state = json!({"testing":app.running_unit_tests().unwrap(),"file":file.to_string_lossy(),"pail":snapshot.get("PAIL_TESTS"),"storage":app.storage_path("").to_string_lossy(),"helper":openrust::support::storage_path("").unwrap().to_string_lossy(),"binding":app.container().get::<PathBuf>("path.storage").unwrap().to_string_lossy()});
            std::fs::write(app.base_path().join("state.json"), state.to_string()).unwrap();
            output.comment("provider command passed");
            Ok(0)
        })
    }
}
fn commands(artisan: &mut openrust::console::Application) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(Emit(CommandDefinition::new(clap::Command::new(
        "provider:emit",
    )))))
}
fn application(root: PathBuf) -> Result<Application, SetupError> {
    let override_path = std::fs::read_to_string(root.join("storage-override")).ok();
    let app = Application::configure(root)
        .with_config(config)
        .with_providers(vec![Box::new(PailServiceProvider)])
        .with_commands(commands)
        .with_routing(Routing::default().web(routes))
        .create()?;
    if let Some(path) = override_path {
        app.use_storage_path(path)?;
    }
    Ok(app)
}

fn create() -> Result<Application, SetupError> {
    application(std::env::var_os("APP_BASE_PATH").unwrap().into())
}
fn clean(command: &mut tokio::process::Command) {
    for key in [
        "APP_ENV",
        "APP_STORAGE_PATH",
        "PAIL_TESTS",
        "VAPOR_SSM_PATH",
        "APP_CONFIG_CACHE",
        "APP_PACKAGES_CACHE",
        "APP_ROUTES_CACHE",
        "APP_EVENTS_CACHE",
    ] {
        command.env_remove(key);
    }
}
async fn artisan(root: &Root, args: &[&str], overrides: &[(&str, &str)]) {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .arg("artisan")
        .args(args)
        .env("APP_BASE_PATH", &root.0)
        .kill_on_drop(true);
    clean(&mut command);
    for (key, value) in overrides {
        command.env(key, value);
    }
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("provider command passed"));
}
async fn http(root: &Root, enabled: bool, environment: &str) {
    let mut app = application(root.0.clone()).unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    app.container()
        .before_resolving(Some("env".into()), move |_, _| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert!(!app.running_unit_tests().unwrap());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!openrust::contracts::foundation::Application::running_unit_tests(&app).unwrap());
    app.boot().await.unwrap();
    assert_eq!(app.running_unit_tests().unwrap(), environment == "testing");
    assert_eq!(
        openrust::contracts::foundation::Application::running_unit_tests(&app).unwrap(),
        environment == "testing"
    );
    assert!(
        !app.container()
            .resolved(ServiceKey::typed::<Handler>())
            .unwrap()
    );
    assert!(
        !app.container()
            .resolved(ServiceKey::typed::<Files>())
            .unwrap()
    );
    assert!(app.container().bound::<Files>());
    assert!(app.container().bound::<Handler>());
    let snapshot = app.container().make::<Environment>().unwrap();
    assert_eq!(snapshot.get_utf8("APP_ENV").unwrap(), Some(environment));
    assert_eq!(
        app.container()
            .get::<PathBuf>("env.file")
            .unwrap()
            .as_path(),
        root.0.join(".env")
    );
    if environment == "local" {
        app.config()
            .unwrap()
            .set("app.env", json!("testing"))
            .unwrap();
        assert!(!app.running_unit_tests().unwrap());
        for (name, expected) in [
            ("testing", true),
            ("TESTING", false),
            ("testing-local", false),
            ("production", false),
        ] {
            app.container()
                .instance_named::<str>("env", Arc::from(name))
                .unwrap();
            assert_eq!(app.running_unit_tests().unwrap(), expected);
        }
        app.container()
            .singleton_named::<str, _>("env", |_| {
                Err(openrust::contracts::container::BindingResolutionError::ScopeClosed)
            })
            .unwrap();
        assert!(matches!(
            app.running_unit_tests(),
            Err(openrust::contracts::container::BindingResolutionError::ScopeClosed)
        ));
        app.container()
            .instance_named::<str>("env", Arc::from(environment))
            .unwrap();
        app.config()
            .unwrap()
            .set("app.env", json!(environment))
            .unwrap();
    }
    let mut kernel = Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert!(
        String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    server.abort();
    let _ = server.await;
    let captured = root.records();
    assert_eq!(captured.len(), usize::from(enabled));
    if enabled {
        assert_eq!(captured[0]["context"]["__pail"]["origin"]["type"], "http");
    }
}
async fn storage_http(root: &Root, directory: &Path, late: bool) {
    File::new(directory.join("pail/session.pail"))
        .create()
        .unwrap();
    let mut app = application(root.0.clone()).unwrap();
    app.boot().await.unwrap();
    if late {
        app.use_storage_path(directory).unwrap();
        std::fs::write(
            root.0.join("storage-override"),
            directory.to_string_lossy().as_bytes(),
        )
        .unwrap();
    }
    assert_eq!(app.storage_path(""), directory);
    if !late {
        assert_eq!(
            app.config().unwrap().get("app.observed_storage").unwrap(),
            json!(directory)
        );
    }
    assert_eq!(
        openrust::contracts::foundation::Application::storage_path(&app, "pail"),
        directory.join("pail")
    );
    let mut kernel = Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains(directory.to_str().unwrap()));
    server.abort();
    let _ = server.await;
    artisan(root, &["provider:emit"], &[]).await;
    let rows = std::fs::read_to_string(directory.join("pail/session.pail")).unwrap();
    assert_eq!(rows.lines().count(), 2);
    assert!(root.records().is_empty());
    let state: Value =
        serde_json::from_slice(&std::fs::read(root.0.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["storage"], directory.to_str().unwrap());
    assert_eq!(state["helper"], state["storage"]);
}
async fn storage_paths() {
    let root = Root::new("local", None, None);
    let app = application(root.0.clone()).unwrap();
    assert_eq!(app.storage_path(""), root.0.join("storage"));
    let separator = std::path::MAIN_SEPARATOR;
    let base = format!("relative{separator}{separator}root{separator}");
    app.use_storage_path(base.clone()).unwrap();
    assert_eq!(
        app.storage_path(&format!("{separator}child")).as_os_str(),
        std::ffi::OsStr::new(&format!("{base}{separator}child"))
    );
    assert_eq!(
        app.storage_path(&format!("..{separator}child")).as_os_str(),
        std::ffi::OsStr::new(&format!("{base}{separator}..{separator}child"))
    );
    assert_eq!(
        app.storage_path("0").as_os_str(),
        std::ffi::OsStr::new(&format!("{base}{separator}0"))
    );
    app.use_storage_path("").unwrap();
    assert_eq!(app.storage_path(""), root.0.join("storage"));
    assert!(
        app.container()
            .get::<PathBuf>("path.storage")
            .unwrap()
            .as_os_str()
            .is_empty()
    );
    app.use_storage_path("0").unwrap();
    assert_eq!(app.storage_path(""), root.0.join("storage"));
    app.container()
        .rebinding::<PathBuf, _>("path.storage", |_, _| {
            Err(openrust::contracts::container::BindingResolutionError::ScopeClosed)
        })
        .unwrap();
    assert!(matches!(
        app.use_storage_path("after-error"),
        Err(openrust::contracts::container::BindingResolutionError::ScopeClosed)
    ));
    assert_eq!(app.storage_path(""), PathBuf::from("after-error"));
    let first = Root::new("local", None, None);
    let a = first.0.join("自定义-a");
    std::fs::write(
        first.0.join(".env"),
        format!(
            "APP_ENV=local\nAPP_STORAGE_PATH={}\n",
            serde_json::to_string(&a.to_string_lossy()).unwrap()
        ),
    )
    .unwrap();
    let second = Root::new("local", None, None);
    let b = second.0.join("custom-b");
    tokio::join!(
        storage_http(&first, &a, false),
        storage_http(&second, &b, true)
    );
    let mut app_a = application(first.0.clone()).unwrap();
    app_a.boot().await.unwrap();
    let mut app_b = application(second.0.clone()).unwrap();
    app_b.boot().await.unwrap();
    let files_a = app_a.container().make::<Files>().unwrap().all().unwrap();
    let files_b = app_b.container().make::<Files>().unwrap().all().unwrap();
    assert_eq!(files_a[0].path(), a.join("pail/session.pail"));
    assert_eq!(files_b[0].path(), b.join("pail/session.pail"));
    for value in ["", "0"] {
        let native = Root::new("testing", None, None);
        std::fs::write(
            native.0.join(".env"),
            format!(
                "APP_ENV=testing\nAPP_STORAGE_PATH={}\n",
                serde_json::to_string(value).unwrap()
            ),
        )
        .unwrap();
        let mut app = application(native.0.clone()).unwrap();
        app.boot().await.unwrap();
        assert_eq!(
            app.storage_path("").as_os_str(),
            std::ffi::OsStr::new(value)
        );
        assert_eq!(
            app.storage_path("pail").as_os_str(),
            std::ffi::OsStr::new(&format!("{value}{}pail", std::path::MAIN_SEPARATOR))
        );
    }
    let process = first.0.join("process");
    File::new(process.join("pail/session.pail"))
        .create()
        .unwrap();
    artisan(
        &first,
        &["provider:emit"],
        &[("APP_STORAGE_PATH", process.to_str().unwrap())],
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(process.join("pail/session.pail"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    std::fs::write(first.0.join("storage-override"), a.to_str().unwrap()).unwrap();
    artisan(
        &first,
        &["provider:emit"],
        &[("APP_STORAGE_PATH", process.to_str().unwrap())],
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(a.join("pail/session.pail"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    println!(
        "Application storage paths, native joining, overrides and Pail HTTP/Artisan isolation passed"
    );
}
async fn verify() {
    let cases = [
        ("local", None, None, true),
        ("production", None, None, true),
        ("testing", None, None, false),
        ("TESTING", None, None, true),
        ("testing", Some("false"), None, false),
        ("testing", Some("(false)"), None, false),
        ("testing", Some("0"), None, false),
        ("testing", Some(""), None, false),
        ("testing", Some("null"), None, false),
        ("testing", Some("(EMPTY)"), None, false),
        ("testing", Some("true"), None, true),
        ("testing", Some("(TRUE)"), None, true),
        ("testing", Some("custom"), None, true),
        ("testing", Some("'0'"), None, false),
        ("testing", Some("\"0\""), None, false),
        ("testing", Some("\"false\""), None, true),
        ("local", None, Some("/vapor/path"), false),
        ("local", None, Some("false"), false),
        ("local", None, Some("0"), true),
        ("local", None, Some(""), true),
        ("local", Some("false"), Some("/vapor/path"), false),
        ("testing", Some("true"), Some("/vapor/path"), true),
    ];
    storage_paths().await;
    let mut roots = Vec::new();
    for (environment, pail, vapor, enabled) in cases {
        let root = Root::new(environment, pail, vapor);
        http(&root, enabled, environment).await;
        artisan(&root, &["provider:emit"], &[]).await;
        assert_eq!(
            root.records().len(),
            2 * usize::from(enabled),
            "{environment} {pail:?} {vapor:?}"
        );
        let original = std::fs::read_to_string(root.0.join("original.log")).unwrap();
        assert!(
            original.contains("provider HTTP probe") && original.contains("provider Artisan probe")
        );
        roots.push(root);
    }
    for (root, (_, _, _, enabled)) in roots.iter().zip(cases) {
        assert_eq!(root.records().len(), 2 * usize::from(enabled));
    }
    let parallel_disabled = Root::new("testing", None, None);
    let parallel_enabled = Root::new("testing", Some("true"), None);
    tokio::join!(
        http(&parallel_disabled, false, "testing"),
        http(&parallel_enabled, true, "testing")
    );
    assert!(parallel_disabled.records().is_empty());
    assert_eq!(parallel_enabled.records().len(), 1);
    let broken = Root::new("local", None, None);
    let mut app = application(broken.0.clone()).unwrap();
    app.boot().await.unwrap();
    app.container()
        .singleton::<Handler, _>(|_| {
            Err(openrust::contracts::container::BindingResolutionError::ScopeClosed)
        })
        .unwrap();
    let logger = app.container().make::<openrust::log::LogManager>().unwrap();
    assert!(matches!(
        logger.info("written before lazy Pail failure", &LogContext::new()),
        Err(openrust::log::LogError::Unavailable(_))
    ));
    assert!(
        std::fs::read_to_string(broken.0.join("original.log"))
            .unwrap()
            .contains("written before lazy Pail failure")
    );
    assert!(broken.records().is_empty());
    let selected = Root::new("local", None, None);
    std::fs::write(selected.0.join(".env.testing"), "APP_ENV=local\n").unwrap();
    artisan(&selected, &["provider:emit", "--env=testing"], &[]).await;
    assert!(selected.records().is_empty());
    let state: Value =
        serde_json::from_slice(&std::fs::read(selected.0.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["testing"], true);
    assert!(state["file"].as_str().unwrap().ends_with(".env.testing"));
    std::fs::write(
        selected.0.join(".env.testing"),
        "APP_ENV=local\nPAIL_TESTS=true\n",
    )
    .unwrap();
    artisan(&selected, &["provider:emit", "--env=testing"], &[]).await;
    assert_eq!(selected.records().len(), 1);
    artisan(
        &selected,
        &["provider:emit", "--env=testing"],
        &[("PAIL_TESTS", "false")],
    )
    .await;
    assert_eq!(selected.records().len(), 1);
    artisan(
        &selected,
        &["provider:emit", "--env=testing"],
        &[("PAIL_TESTS", "'false'")],
    )
    .await;
    assert_eq!(selected.records().len(), 2);
    let cached = Root::new("local", Some("true"), None);
    std::fs::create_dir_all(cached.0.join("storage/framework")).unwrap();
    let values = json!({"app":{"env":"testing","host":"127.0.0.1","port":0},"logging":{"default":"single","channels":{"single":{"driver":"single","path":"original.log"}}}});
    std::fs::write(
        cached.0.join("storage/framework/config.json"),
        values.to_string(),
    )
    .unwrap();
    artisan(&cached, &["provider:emit"], &[]).await;
    assert!(cached.records().is_empty());
    let state: Value =
        serde_json::from_slice(&std::fs::read(cached.0.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["testing"], true);
    assert!(state["pail"].is_null());
    artisan(&cached, &["provider:emit"], &[("PAIL_TESTS", "true")]).await;
    assert_eq!(cached.records().len(), 1);
    println!(
        "Pail provider 22 real HTTP/Artisan cases, selected env, overrides and app isolation passed"
    );
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os().nth(1) == Some(OsString::from("artisan")) {
        return run_application(create).await;
    }
    if std::env::args_os().nth(1) == Some(OsString::from("verify")) {
        verify().await;
        return ExitCode::SUCCESS;
    }
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
    child.arg("verify").kill_on_drop(true);
    clean(&mut child);
    let result = tokio::time::timeout(Duration::from_secs(90), child.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8_lossy(&result.stdout));
    ExitCode::SUCCESS
}
