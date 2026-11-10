use std::{
    collections::BTreeMap,
    fs, io,
    net::TcpListener,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{
        Application, BootstrapError, bootstrap::LoadEnvironmentVariables, configuration::Routing,
        http::Kernel,
    },
    http::{Body, Request, StatusCode},
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;

#[path = "../routes/web.rs"]
mod web;

#[path = "../config/mod.rs"]
mod config;

struct TestRoot(PathBuf);

impl TestRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("openrust-startup-{}-{name}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn process_entry_selects_a_kernel_before_bootstrap() {
    let root = TestRoot::new("process-entry");
    let binary = env!("CARGO_BIN_EXE_main");
    let run = |arguments: &[&str]| {
        Command::new(binary)
            .current_dir(&root.0)
            .env("APP_BASE_PATH", &root.0)
            .args(arguments)
            .output()
            .unwrap()
    };
    let unknown = ["unknown", "artisan", "list"];
    let output = run(&unknown);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let name = std::path::Path::new(binary).file_name().unwrap();
    let expected = format!("Usage: {} [artisan [COMMAND]]", name.to_string_lossy());
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim_end(), expected);

    fs::write(root.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let output = run(&unknown);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim_end(), expected);
    for arguments in [&[][..], &["artisan", "list"][..]] {
        let output = run(arguments);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.starts_with("Application startup failed: configuration failed"));
        assert!(!error.contains("Usage:"));
    }
}

#[test]
fn compiled_binaries_use_the_declared_root_independent_of_working_directory() {
    let working = TestRoot::new("foreign-cwd");
    fs::write(working.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let installed = TestRoot::new("installed-app");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_main"));
    let relocated = installed.0.join(binary.file_name().unwrap());
    fs::copy(binary, &relocated).unwrap();

    let run = |executable: &std::path::Path, root: Option<&std::path::Path>| {
        let mut command = Command::new(executable);
        command.env_remove("APP_BASE_PATH");
        if let Some(root) = root {
            command.env("APP_BASE_PATH", root);
        }
        command
            .current_dir(&working.0)
            .args(["artisan", "list"])
            .output()
            .unwrap()
    };
    assert!(run(binary, None).status.success());
    assert!(run(&relocated, Some(&installed.0)).status.success());

    fs::write(installed.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let output = run(&relocated, None);
    if cfg!(feature = "standalone") {
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("configuration failed"));
    } else {
        assert!(output.status.success());
    }
    let output = run(&relocated, Some(&installed.0));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("configuration failed"));

    let output = run(&relocated, Some(std::path::Path::new("relative")));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expected an absolute application root")
    );
}

struct First(Arc<Mutex<Vec<&'static str>>>);
struct Second(Arc<Mutex<Vec<&'static str>>>);

impl ServiceProvider for First {
    fn register(&mut self, app: &Application) -> ProviderResult {
        assert_eq!(app.config().get("app.name"), Some(json!("test app")));
        app.container().instance(self.0.clone());
        self.0.lock().unwrap().push("first.register");
        Ok(())
    }

    fn boot(&mut self, app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        let service = app.container().make::<str>();
        Box::pin(async move {
            assert_eq!(&*service?, "service from second");
            self.0.lock().unwrap().push("first.boot");
            Ok(())
        })
    }
}

impl ServiceProvider for Second {
    fn register(&mut self, app: &Application) -> ProviderResult {
        app.container()
            .instance::<str>(Arc::from("service from second"));
        self.0.lock().unwrap().push("second.register");
        Ok(())
    }

    fn boot(&mut self, _app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        Box::pin(async move {
            self.0.lock().unwrap().push("second.boot");
            Ok(())
        })
    }
}

#[tokio::test]
async fn all_providers_register_before_boot_and_success_is_idempotent() {
    let root = TestRoot::new("providers");
    let events = Arc::new(Mutex::new(Vec::new()));
    let middleware_events = events.clone();
    let application = Application::configure(root.0.clone())
        .withConfig(|_, _| Ok(BTreeMap::from([("app".into(), json!({"name":"test app"}))])))
        .withProviders(vec![
            Box::new(First(events.clone())),
            Box::new(Second(events.clone())),
            Box::new(Second(events.clone())),
        ])
        .withMiddleware(move |_| middleware_events.lock().unwrap().push("middleware"))
        .withRouting(Routing::new(|app| {
            assert_eq!(
                &*app.container().make::<str>().unwrap(),
                "service from second"
            );
            app.container()
                .make::<Mutex<Vec<&'static str>>>()
                .unwrap()
                .lock()
                .unwrap()
                .push("routes");
            Ok(openrust::routing::Router::new())
        }))
        .create()
        .unwrap();
    assert_eq!(application.base_path(), root.0.canonicalize().unwrap());
    assert!(!application.is_booted());
    assert!(events.lock().unwrap().is_empty());
    let repository = application
        .container()
        .make::<dyn openrust::contracts::config::Repository>()
        .unwrap();
    assert!(!repository.has("app.name"));
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    kernel.bootstrap().await.unwrap();
    assert!(kernel.get_application().is_booted());
    assert_eq!(repository.get("app.name"), Some(json!("test app")));
    repository.set("app.extra", Value::Null).unwrap();
    assert!(kernel.get_application().config().has("app.extra"));
    assert_eq!(
        *events.lock().unwrap(),
        [
            "middleware",
            "first.register",
            "second.register",
            "first.boot",
            "second.boot",
            "routes"
        ]
    );
}

struct FailingProvider;

impl ServiceProvider for FailingProvider {
    fn register(&mut self, _app: &Application) -> ProviderResult {
        Ok(())
    }

    fn boot(&mut self, _app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        Box::pin(async { Err(io::Error::other("private probe").into()) })
    }
}

fn listener_config(
    base: &std::path::Path,
    _: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let port: u16 = fs::read_to_string(base.join("port"))
        .unwrap()
        .parse()
        .unwrap();
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"host":"127.0.0.1", "port":port}),
    )]))
}

#[tokio::test]
async fn start_bootstraps_through_the_http_kernel() {
    let root = TestRoot::new("start-before-boot");
    let application = Application::configure(root.0.clone())
        .withProviders(vec![Box::new(FailingProvider)])
        .create()
        .unwrap();

    let error = application.start().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::ProviderBootFailed { .. })
    ));
}

#[tokio::test]
async fn failed_provider_prevents_listening_and_is_not_retried() {
    let root = TestRoot::new("provider-failure");
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    fs::write(
        root.0.join("port"),
        occupied.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let application = Application::configure(root.0.clone())
        .withConfig(listener_config)
        .withProviders(vec![Box::new(FailingProvider)])
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);

    // An occupied port would fail first if the real listener bound before bootstrap.
    let error = kernel.listen().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::ProviderBootFailed { .. })
    ));
    assert!(error.to_string().contains("FailingProvider"));
    assert!(!error.to_string().contains("private probe"));
    assert!(!kernel.get_application().is_booted());
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::PreviousBootFailed)
    ));
    let (parts, _) = http_request().into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    assert_eq!(
        kernel.handle(&mut request).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

fn http_request() -> axum::extract::Request {
    axum::extract::Request::new(Body::empty())
}

#[tokio::test]
async fn welcome_route_reads_the_named_view_from_the_application_root() {
    let root = TestRoot::new("named-view");
    let views = root.0.join("resources/views");
    let custom_views = root.0.join("custom-views");
    fs::create_dir_all(&views).unwrap();
    fs::create_dir_all(&custom_views).unwrap();
    let welcome = views.join("welcome.html");
    let custom_welcome = custom_views.join("welcome.html");
    fs::write(&welcome, "default welcome").unwrap();
    fs::write(&custom_welcome, "custom welcome").unwrap();

    let application = Application::configure(root.0.clone())
        .withConfig(|base, env| {
            let mut config = config::load(base, env)?;
            config.get_mut("view").unwrap()["paths"] = json!(["custom-views", "resources/views"]);
            Ok(config)
        })
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    let (parts, body) = http_request().into_parts();
    let (_, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "custom welcome"
    );
    fs::remove_file(custom_welcome).unwrap();

    for html in ["<h1>Welcome</h1>", "<h1>Updated</h1>"] {
        fs::write(&welcome, html).unwrap();
        let (parts, body) = http_request().into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let (parts, body) = response.into_parts();
        assert_eq!(parts.headers["content-type"], "text/html; charset=utf-8");
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), html);
    }

    fs::remove_file(welcome).unwrap();
    let (parts, body) = http_request().into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn public_mount_serves_files_after_application_routes() {
    use openrust::{
        contracts::routing::Registrar as _,
        http::{HttpError, HttpFuture, Method, Response},
        routing::Router,
        support::facades::Route,
        view::view,
    };

    fn not_found(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Err(HttpError::NotFound) })
    }

    let root = TestRoot::new("static-mount");
    let public = root.0.join("public");
    let public_storage = root.0.join("storage/app/public");
    let private_storage = root.0.join("storage/app/private");
    fs::create_dir_all(public.join("css")).unwrap();
    fs::create_dir_all(&public_storage).unwrap();
    fs::create_dir_all(&private_storage).unwrap();
    fs::write(public_storage.join("avatar.png"), "public avatar").unwrap();
    fs::create_dir_all(public.join("storage")).unwrap();
    fs::write(public.join("storage/avatar.png"), "website avatar").unwrap();
    fs::write(public_storage.join("protected.txt"), "disk mount shadow").unwrap();
    fs::write(private_storage.join("secret.txt"), "private file").unwrap();
    fs::create_dir_all(root.0.join("resources/views")).unwrap();
    fs::write(
        root.0.join("resources/views/welcome.html"),
        "application home",
    )
    .unwrap();
    fs::write(root.0.join("outside.txt"), "outside the mount").unwrap();
    for (path, body) in [
        ("logo.png", "image"),
        ("css/app.css", "body{}"),
        ("css/index.html", "directory index"),
        ("index.html", "static home"),
        ("protected.txt", "static shadow"),
        ("post-only.txt", "static shadow"),
        ("你好.txt", "encoded name"),
    ] {
        fs::write(public.join(path), body).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../outside.txt", public.join("external.txt")).unwrap();
        std::os::unix::fs::symlink("../private/secret.txt", public_storage.join("linked.txt"))
            .unwrap();
    }

    let application = Application::configure(root.0.clone())
        .withConfig(config::load)
        .withRouting(
            Routing::new(|_| {
                let mut router = Router::new();
                router.add_route(&[Method::POST], "/post-only.txt", Arc::new(not_found))?;
                Ok(router)
            })
            .web(|| {
                Route::staticDisk("public")?;
                Route::get("/", || view("welcome"))?;
                Route::staticPublic()?;
                Route::get("/protected.txt", not_found)?;
                Route::get("/storage/protected.txt", not_found)?;
                Route::r#static("/assets", "./public/css")
            })
            .health("/up"),
        )
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    let config = kernel.get_application().config();
    let app_url = config.get("app.url").unwrap();
    let disk_url = format!(
        "{}/storage",
        app_url.as_str().unwrap().trim_end_matches('/')
    );
    assert_eq!(
        config.get("filesystems.disks.public.url"),
        Some(json!(disk_url))
    );

    for (method, uri, body, mime) in [
        (
            Method::GET,
            "/",
            "application home",
            "text/html; charset=utf-8",
        ),
        (Method::GET, "/logo.png", "image", "image/png"),
        (
            Method::GET,
            "/storage/avatar.png",
            "public avatar",
            "image/png",
        ),
        (Method::GET, "/css/app.css?version=1", "body{}", "text/css"),
        (Method::HEAD, "/css/app.css", "", "text/css"),
        (Method::GET, "/assets/app.css", "body{}", "text/css"),
        (
            Method::GET,
            "/%E4%BD%A0%E5%A5%BD.txt",
            "encoded name",
            "text/plain",
        ),
    ] {
        let (parts, request_body) = axum::http::Request::builder()
            .method(method.clone())
            .uri(uri)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel
            .handle(&mut Request::from_parts(parts, request_body))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let (parts, response_body) = response.into_parts();
        assert_eq!(parts.headers["content-type"], mime, "{uri}");
        if method == Method::HEAD {
            assert_eq!(parts.headers["content-length"], "6");
        }
        assert_eq!(
            axum::body::to_bytes(response_body, 1024).await.unwrap(),
            body,
            "{uri}"
        );
    }

    for (method, uri, status) in [
        (Method::GET, "/up", StatusCode::OK),
        (Method::GET, "/css", StatusCode::NOT_FOUND),
        (Method::GET, "/css/", StatusCode::NOT_FOUND),
        (Method::GET, "/logo.png/", StatusCode::NOT_FOUND),
        (Method::GET, "/public/logo.png", StatusCode::NOT_FOUND),
        (Method::GET, "/avatar.png", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/secret.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/linked.txt", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/storage/%2e%2e/private/secret.txt",
            StatusCode::NOT_FOUND,
        ),
        (Method::GET, "/missing.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/protected.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/protected.txt", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/post-only.txt",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (Method::GET, "/assets-other/app.css", StatusCode::NOT_FOUND),
        (Method::GET, "/../outside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/%2e%2e/outside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/..%5coutside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/external.txt", StatusCode::NOT_FOUND),
        (Method::POST, "/logo.png", StatusCode::NOT_FOUND),
    ] {
        let (parts, body) = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), status, "{uri}");
        if status == StatusCode::METHOD_NOT_ALLOWED {
            assert_eq!(response.headers()["allow"], "POST");
        }
    }

    let application = Application::configure(root.0.clone())
        .withConfig(config::load)
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut reversed = Kernel::new(application);
    reversed.bootstrap().await.unwrap();
    let (parts, body) = axum::http::Request::get("/storage/avatar.png")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = reversed.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, body) = response.into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "website avatar"
    );

    let app = Application::configure(root.0.clone()).create().unwrap();
    let mut unmounted = Kernel::new(app);
    unmounted.bootstrap().await.unwrap();
    let (parts, body) = axum::http::Request::get("/logo.png")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = unmounted
        .handle(&mut Request::from_parts(parts, body))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_mount_revalidates_get_and_head_using_file_modification_time() {
    use std::time::{Duration, UNIX_EPOCH};

    let root = TestRoot::new("static-revalidation");
    let public = root.0.join("public");
    fs::create_dir(&public).unwrap();
    let path = public.join("app.css");
    let set_file = |body: &str, millis: u64| {
        fs::write(&path, body).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)),
            )
            .unwrap();
    };
    set_file("body{}", 1_700_000_000_123);
    let application = Application::configure(root.0.clone())
        .withConfig(config::load)
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    const MODIFIED: &str = "Tue, 14 Nov 2023 22:13:20 GMT";
    for (method, since, tag, status, content) in [
        ("GET", None, None, StatusCode::OK, "body{}"),
        ("GET", Some(MODIFIED), None, StatusCode::NOT_MODIFIED, ""),
        ("HEAD", Some(MODIFIED), None, StatusCode::NOT_MODIFIED, ""),
        ("GET", Some("invalid"), None, StatusCode::OK, "body{}"),
        (
            "GET",
            Some(MODIFIED),
            Some("\"other\""),
            StatusCode::OK,
            "body{}",
        ),
        ("GET", None, Some("*"), StatusCode::NOT_MODIFIED, ""),
    ] {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri("/app.css");
        if let Some(since) = since {
            request = request.header("if-modified-since", since);
        }
        if let Some(tag) = tag {
            request = request.header("if-none-match", tag);
        }
        let (parts, body) = request.body(Body::empty()).unwrap().into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), status, "{method} {since:?} {tag:?}");
        assert_eq!(response.headers()["last-modified"], MODIFIED);
        if status == StatusCode::NOT_MODIFIED {
            assert!(!response.headers().contains_key("content-length"));
            assert!(!response.headers().contains_key("content-type"));
        }
        let (_, body) = response.into_parts();
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), content);
    }

    set_file("body{color:red}", 1_700_000_002_123);
    let (parts, body) = axum::http::Request::get("/app.css")
        .header("if-modified-since", MODIFIED)
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["last-modified"],
        "Tue, 14 Nov 2023 22:13:22 GMT"
    );
    let (_, body) = response.into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "body{color:red}"
    );
}

#[tokio::test]
async fn public_mounts_follow_the_application_and_disk_configuration() {
    use openrust::support::facades::Route;

    let root = TestRoot::new("configured-mounts");
    for (directory, contents) in [
        ("assets", "application assets"),
        ("uploads", "public disk"),
        ("override", "explicit application path"),
        ("public", "old default"),
        ("storage/app/public", "old disk default"),
    ] {
        let directory = root.0.join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("probe.txt"), contents).unwrap();
    }
    let environment = root.0.join(".env");
    let settings = "APP_URL=http://localhost:8000\nPUBLIC_PATH=assets\nPUBLIC_URL_PREFIX=/assets\nPUBLIC_STORAGE_ROOT=uploads\n";
    let application = |register: fn() -> Result<(), openrust::routing::RoutingError>| {
        Application::configure(root.0.clone())
            .withConfig(config::load)
            .withRouting(Routing::default().web(register))
            .create()
            .unwrap()
    };

    async fn check(kernel: &Kernel, path: &str, expected: Option<&str>) {
        let (parts, body) = axum::http::Request::get(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        if let Some(expected) = expected {
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let (_, body) = response.into_parts();
            assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), expected);
        } else {
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    fs::write(
        &environment,
        format!(
            "{}PUBLIC_STORAGE_URL=https://LOCALHOST:443/storage/tmp/\n",
            settings.replace("http://localhost:8000", "https://localhost")
        ),
    )
    .unwrap();
    let mut kernel = Kernel::new(application(Route::staticPublic));
    kernel.bootstrap().await.unwrap();
    check(&kernel, "/assets/probe.txt", Some("application assets")).await;
    check(&kernel, "/storage/tmp/probe.txt", None).await;
    check(&kernel, "/storage/probe.txt", None).await;

    let mut kernel = Kernel::new(application(web::register));
    kernel.bootstrap().await.unwrap();
    check(&kernel, "/assets/probe.txt", Some("application assets")).await;
    check(&kernel, "/storage/tmp/probe.txt", Some("public disk")).await;
    check(&kernel, "/storage/tmp-other/probe.txt", None).await;
    check(&kernel, "/probe.txt", None).await;
    check(&kernel, "/storage/probe.txt", None).await;

    let cdn = format!("{settings}PUBLIC_STORAGE_URL=https://cdn.example/media\n");
    fs::write(&environment, &cdn).unwrap();
    let mut kernel = Kernel::new(application(web::register));
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::RoutingFailed { .. })
    ));

    fs::write(
        &environment,
        format!("{settings}PUBLIC_STORAGE_URL=/local-media\n"),
    )
    .unwrap();
    let mut app = application(web::register);
    app.usePublicPath("override");
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    check(
        &kernel,
        "/assets/probe.txt",
        Some("explicit application path"),
    )
    .await;
    check(&kernel, "/local-media/probe.txt", Some("public disk")).await;
    check(&kernel, "/storage/probe.txt", None).await;

    fs::write(&environment, "APP_PORT=9081\nPUBLIC_STORAGE_ROOT=uploads\n").unwrap();
    let mut kernel = Kernel::new(application(web::register));
    kernel.bootstrap().await.unwrap();
    let config = kernel.get_application().config();
    assert_eq!(config.get("app.port"), Some(json!(9081)));
    assert_eq!(config.get("app.url"), Some(json!("http://localhost:9081")));
    assert_eq!(
        config.get("filesystems.disks.public.url"),
        Some(json!("http://localhost:9081/storage"))
    );
    check(&kernel, "/storage/probe.txt", Some("public disk")).await;
}

#[test]
fn environment_file_selection_and_redaction() {
    let root = TestRoot::new("environment");
    fs::write(root.0.join(".env"), "OPENRUST_TEST_PROBE=default\n").unwrap();
    fs::write(root.0.join(".env.test"), "OPENRUST_TEST_PROBE=selected\n").unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, Some("test")).unwrap();
    assert_eq!(environment.get("OPENRUST_TEST_PROBE"), Some("selected"));
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, Some("missing")).unwrap();
    assert_eq!(environment.get("OPENRUST_TEST_PROBE"), Some("default"));
    assert!(matches!(
        LoadEnvironmentVariables::bootstrap(&root.0, Some("../test")),
        Err(ConfigError::InvalidEnvironmentName)
    ));
    fs::write(root.0.join(".env.test"), "APP_KEY=\"private-probe\n").unwrap();
    let error = LoadEnvironmentVariables::bootstrap(&root.0, Some("test"))
        .err()
        .unwrap();
    assert!(matches!(error, ConfigError::InvalidEnvironmentFile { .. }));
    assert!(!error.to_string().contains("private-probe"));
}

#[tokio::test]
async fn invalid_routes_fail_bootstrap_before_the_listener_opens() {
    let root = TestRoot::new("invalid-route");
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    fs::write(
        root.0.join("port"),
        occupied.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let application = Application::configure(root.0.clone())
        .withConfig(listener_config)
        .withRouting(Routing::new(|_| {
            use openrust::{contracts::routing::Registrar as _, routing::Router};
            let mut router = Router::new();
            router.add_route(&[], "/", Arc::new(unreachable_handler))?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);

    let error = kernel.listen().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::RoutingFailed { .. })
    ));
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::PreviousBootFailed)
    ));
}

fn unreachable_handler(
    _: &mut Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    unreachable!("invalid route must not be dispatched")
}
