use openrust::{
    config::Repository,
    log::{FormattableHandler, Formatter, LogContext, LogError, LogManager, LogRecord, LogResult},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
struct TextHandler {
    path: PathBuf,
    calls: Arc<AtomicUsize>,
    fail_first: bool,
}
impl FormattableHandler for TextHandler {
    fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_first && call == 0 {
            return Err(LogError::InvalidConfiguration);
        }
        Ok(Arc::new(|record| Ok(format!("OWN:{}\n", record.message()))))
    }
    fn write(&self, _: &LogRecord<'_>, formatted: &str) -> LogResult<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(formatted.as_bytes())?;
        Ok(())
    }
}
fn register(manager: &LogManager, root: &Path, calls: Arc<AtomicUsize>) {
    let root = root.to_owned();
    manager.register_formattable_handler("text", move |options| {
        if options["constructor_error"] == true {
            return Err(LogError::InvalidConfiguration);
        };
        let path = options["path"]
            .as_str()
            .ok_or(LogError::InvalidConfiguration)?;
        Ok(Arc::new(TextHandler {
            path: root.join(path),
            calls: calls.clone(),
            fail_first: options["fail_first"] == true,
        }))
    });
}
fn manager(root: &Path) -> LogManager {
    LogManager::new(Arc::new(Repository::new(BTreeMap::new())), root)
}
#[test]
fn own_formatter_is_lazy_selected_only_by_default_and_errors_retry_without_replacing_the_channel() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let calls = Arc::new(AtomicUsize::new(0));
    register(&manager, root.path(), calls.clone());
    let own=manager.build(&json!({"driver":"monolog","handler":"text","handler_with":{"path":"own","fail_first":true},"formatter":"default"})).unwrap();
    let ordinary=manager.build(&json!({"driver":"monolog","handler":"text","handler_with":{"path":"ordinary","fail_first":true}})).unwrap();
    let explicit=manager.build(&json!({"driver":"monolog","handler":"text","handler_with":{"path":"explicit","fail_first":true},"formatter":"line","formatter_with":{"format":"SELECTED:%message%\n"}})).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    ordinary.info("ordinary", &LogContext::new()).unwrap();
    explicit.info("explicit", &LogContext::new()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    own.info("first fails", &LogContext::new()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!root.path().join("own").exists());
    let emergency = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert!(emergency.contains("first fails"));
    own.info("recovered", &LogContext::new()).unwrap();
    own.info("cached", &LogContext::new()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read_to_string(root.path().join("own")).unwrap(),
        "OWN:recovered\nOWN:cached\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("explicit")).unwrap(),
        "SELECTED:explicit\n"
    );
    assert!(
        std::fs::read_to_string(root.path().join("ordinary"))
            .unwrap()
            .contains("ordinary")
    );
    manager.build(&json!({"driver":"monolog","handler":"text","handler_with":{"path":"bad","constructor_error":true},"formatter":"default"})).unwrap().error("constructor fallback",&LogContext::new()).unwrap();
    assert!(!root.path().join("bad").exists());
    assert!(
        std::fs::read_to_string(root.path().join("storage/logs/app.log"))
            .unwrap()
            .contains("constructor fallback")
    );
}
#[test]
fn legacy_callbacks_retain_the_standard_owned_line_formatter_and_filtered_records_do_not_initialize()
 {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let output = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = output.clone();
    manager.register_handler("callback", move |_| {
        let captured = captured.clone();
        Ok(Arc::new(move |_, text| {
            captured.lock().unwrap().push(text.to_owned());
            Ok(())
        }))
    });
    manager.build(&json!({"driver":"monolog","handler":"callback","formatter":"default","name":"callback"})).unwrap().info("legacy",&LogContext::new()).unwrap();
    let output = output.lock().unwrap();
    assert!(output[0].contains('T') && output[0].ends_with("callback.INFO: legacy [] []\n"));
    drop(output);
    let calls = Arc::new(AtomicUsize::new(0));
    register(&manager, root.path(), calls.clone());
    let filtered=manager.build(&json!({"driver":"monolog","handler":"text","handler_with":{"path":"filtered","level":"error"},"formatter":"default"})).unwrap();
    filtered.info("ignored", &LogContext::new()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!root.path().join("filtered").exists());
}
fn configuration(
    _: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"production","key":"a".repeat(32)}),
        ),
        (
            "session".into(),
            json!({"driver":"array","lottery":[0,100]}),
        ),
        (
            "logging".into(),
            json!({"default":"own","channels":{"own":{"driver":"monolog","handler":"text","handler_with":{"path":"actual.log"},"formatter":"default"}}}),
        ),
    ]))
}
struct Provider;
impl openrust::support::ServiceProvider for Provider {
    fn register<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> openrust::http::HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            use openrust::contracts::{container::Container as _, foundation::Application as _};
            let manager = app.container().make::<LogManager>()?;
            register(
                manager.as_ref(),
                app.base_path(),
                Arc::new(AtomicUsize::new(0)),
            );
            Ok(())
        })
    }
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/log", || {
        openrust::support::facades::Log::info("http", &LogContext::new()).unwrap();
        "ok"
    })?;
    Ok(())
}
fn commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::command("logs:formattable", |_| {
        openrust::support::facades::Log::info("artisan", &LogContext::new()).unwrap();
    })?
    .purpose("验收处理器自带格式")
}
#[tokio::test]
async fn actual_http_and_independent_artisan_use_the_registered_capability() {
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    if let Ok(root) = std::env::var("FORMATTABLE_LOG_ROOT") {
        let app = Application::configure(root.into())
            .with_config(configuration)
            .with_providers(vec![Box::new(Provider)])
            .with_routing(Routing::default().commands(commands))
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["logs:formattable".into()])
                .await
                .unwrap(),
            0
        );
        return;
    }
    use axum::handler::Handler as _;
    let root = tempfile::tempdir().unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_config(configuration)
            .with_providers(vec![Box::new(Provider)])
            .with_routing(Routing::default().web(routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(kernel).into_make_service())
            .await
            .unwrap()
    });
    assert_eq!(
        reqwest::get(format!("http://{address}/log"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "ok"
    );
    let owned = root.path().to_owned();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "actual_http_and_independent_artisan_use_the_registered_capability",
                "--nocapture",
            ])
            .env("FORMATTABLE_LOG_ROOT", owned)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("actual.log")).unwrap(),
        "OWN:http\nOWN:artisan\n"
    );
    assert!(!root.path().join("storage/logs/app.log").exists());
    server.abort();
}
#[test]
fn recursive_default_getters_return_an_error_and_can_recover_on_the_same_native_handle() {
    struct Reentrant {
        target: Arc<std::sync::Mutex<Option<std::sync::Weak<openrust::log::NativeLogger>>>>,
        once: std::sync::atomic::AtomicBool,
        path: PathBuf,
    }
    impl FormattableHandler for Reentrant {
        fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
            if self.once.swap(false, Ordering::SeqCst) {
                let target = self
                    .target
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap()
                    .upgrade()
                    .unwrap();
                target.info("recursive", &LogContext::new())?;
            }
            Ok(Arc::new(|record| {
                Ok(format!("READY:{}\n", record.message()))
            }))
        }
        fn write(&self, _: &LogRecord<'_>, text: &str) -> LogResult<()> {
            std::fs::write(&self.path, text)?;
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let target = Arc::new(std::sync::Mutex::new(None));
    let captured = target.clone();
    let path = root.path().join("recursive");
    let output = path.clone();
    manager.register_formattable_handler("recursive", move |_| {
        Ok(Arc::new(Reentrant {
            target: captured.clone(),
            once: std::sync::atomic::AtomicBool::new(true),
            path: output.clone(),
        }))
    });
    let logger = manager
        .build(&json!({"driver":"monolog","handler":"recursive","formatter":"default"}))
        .unwrap()
        .get_logger();
    *target.lock().unwrap() = Some(Arc::downgrade(&logger));
    fn cyclic(error: &(dyn std::error::Error + 'static), depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        if matches!(
            error.downcast_ref::<LogError>(),
            Some(LogError::CyclicStack)
        ) {
            return true;
        }
        if let Some(error) = error
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
        {
            return cyclic(error, depth + 1);
        }
        error
            .source()
            .is_some_and(|source| cyclic(source, depth + 1))
    }
    let failure = logger.info("outer", &LogContext::new()).unwrap_err();
    assert!(cyclic(&failure, 0), "{failure:?}");
    assert!(!path.exists());
    logger.info("after error", &LogContext::new()).unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "READY:after error\n"
    );
}
#[test]
fn concurrent_first_writes_publish_one_successful_formatter_without_serializing_user_callbacks() {
    struct Racing {
        calls: Arc<AtomicUsize>,
        path: PathBuf,
    }
    impl FormattableHandler for Racing {
        fn get_default_formatter(&self) -> LogResult<Arc<Formatter>> {
            let version = self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::yield_now();
            Ok(Arc::new(move |record| {
                Ok(format!("V{version}:{}\n", record.message()))
            }))
        }
        fn write(&self, _: &LogRecord<'_>, text: &str) -> LogResult<()> {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?
                .write_all(text.as_bytes())?;
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = calls.clone();
    let path = root.path().join("racing");
    let output = path.clone();
    manager.register_formattable_handler("racing", move |_| {
        Ok(Arc::new(Racing {
            calls: captured.clone(),
            path: output.clone(),
        }))
    });
    let logger = manager
        .build(&json!({"driver":"monolog","handler":"racing","formatter":"default"}))
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(16));
    std::thread::scope(|scope| {
        for _ in 0..16 {
            let logger = logger.clone();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                logger.info("accepted", &LogContext::new()).unwrap();
            });
        }
    });
    let text = std::fs::read_to_string(path).unwrap();
    let rows = text.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), 16);
    assert!(rows.iter().all(|line| *line == rows[0]));
    assert!((1..=16).contains(&calls.load(Ordering::SeqCst)));
}
