use openrust::{
    filesystem::{FileError, Filesystem, RequiredFile},
    support::facades::{App, Facade, File},
    testing::TestApp,
};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

async fn fixture() -> TestApp {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn get_require_passes_owned_typed_data_each_time_and_once_keeps_original_result_distinct() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module.rs");
        fs::write(&path, "// 已编译模块的来源路径").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        File::register_module(&path, move |files, name: String| {
            assert!(!name.is_empty());
            assert!(!files.has_macro("not-registered"));
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(format!("hello {name}"))
        })
        .unwrap();
        assert_eq!(
            File::get_require::<String, _>(&path, "first".to_owned()).unwrap(),
            "hello first"
        );
        assert_eq!(
            File::require_once::<String, _>(&path, "skip".to_owned()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
        assert_eq!(
            File::get_require::<String, _>(&path, "again".to_owned()).unwrap(),
            "hello again"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let boolean = dir.path().join("boolean.rs");
        fs::write(&boolean, "").unwrap();
        File::register_module(&boolean, |_, (): ()| Ok(true)).unwrap();
        assert_eq!(
            File::require_once::<bool, _>(&boolean, ()).unwrap(),
            RequiredFile::Value(true)
        );
        assert_eq!(
            File::require_once::<bool, _>(&boolean, ()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
    })
    .await;
}

#[tokio::test]
async fn failures_panic_reentry_and_wrong_types_never_reexecute_once_or_hold_registry_locks() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let failed = dir.path().join("failed.rs");
        fs::write(&failed, "").unwrap();
        File::register_module(&failed, |_, (): ()| -> Result<(), FileError> {
            Err(FileError::UnknownAlgorithm("module-error".into()))
        })
        .unwrap();
        assert!(matches!(
            File::require_once::<(), _>(&failed, ()),
            Err(FileError::UnknownAlgorithm(_))
        ));
        assert_eq!(
            File::require_once::<(), _>(&failed, ()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
        let recursive = dir.path().join("recursive.rs");
        fs::write(&recursive, "").unwrap();
        let recurse = recursive.clone();
        File::register_module(&recursive, move |files, (): ()| {
            assert_eq!(
                files.require_once::<usize, _>(&recurse, ()).unwrap(),
                RequiredFile::AlreadyIncluded
            );
            files.r#macro("registered-during-load", |_, (): ()| Ok(7usize));
            Ok(42usize)
        })
        .unwrap();
        assert_eq!(
            File::require_once::<usize, _>(&recursive, ()).unwrap(),
            RequiredFile::Value(42)
        );
        assert_eq!(
            File::macro_call::<usize, _>("registered-during-load", ()).unwrap(),
            7
        );
        let panic_file = dir.path().join("panic.rs");
        fs::write(&panic_file, "").unwrap();
        File::register_module(&panic_file, |_, (): ()| -> Result<(), FileError> {
            panic!("module-panic")
        })
        .unwrap();
        assert!(std::panic::catch_unwind(|| File::require_once::<(), _>(&panic_file, ())).is_err());
        assert_eq!(
            File::require_once::<(), _>(&panic_file, ()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
        let wrong = dir.path().join("wrong.rs");
        fs::write(&wrong, "").unwrap();
        File::register_module(&wrong, |_, input: String| Ok(input.len())).unwrap();
        assert!(matches!(
            File::get_require::<usize, _>(&wrong, 42u32),
            Err(FileError::MacroType { .. })
        ));
        assert!(matches!(
            File::get_require::<String, _>(&wrong, "correct-input".to_owned()),
            Err(FileError::MacroType { .. })
        ));
    })
    .await;
}

#[tokio::test]
async fn registration_missing_and_removed_files_and_separate_apps_are_not_fabricated_successes() {
    let first = fixture().await;
    let second = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("real.rs");
    fs::write(&path, "").unwrap();
    App::set_facade_application(Some(first.app()), async {
        assert!(matches!(
            File::get_require::<bool, _>(&path, ()),
            Err(FileError::UnregisteredModule(_))
        ));
        assert!(matches!(
            File::get_require::<bool, _>(dir.path().join("missing.rs"), ()),
            Err(FileError::Io { .. })
        ));
        File::register_module(&path, |_, (): ()| Ok(true)).unwrap();
        assert_eq!(
            File::require_once::<bool, _>(&path, ()).unwrap(),
            RequiredFile::Value(true)
        );
        // 替换编译回调不使同一路径变回“未包含”；get_require仍每次执行新回调。
        File::register_module(&path, |_, (): ()| Ok(false)).unwrap();
        assert_eq!(
            File::require_once::<bool, _>(&path, ()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
        assert!(!File::get_require::<bool, _>(&path, ()).unwrap());
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        assert!(matches!(
            File::require_once::<bool, _>(&path, ()),
            Err(FileError::UnregisteredModule(_))
        ));
        File::register_module(&path, |_, (): ()| Ok(99u32)).unwrap();
        assert_eq!(
            File::require_once::<u32, _>(&path, ()).unwrap(),
            RequiredFile::Value(99)
        );
    })
    .await;
    fs::remove_file(&path).unwrap();
    App::set_facade_application(Some(first.app()), async {
        assert!(matches!(
            File::require_once::<bool, _>(&path, ()),
            Err(FileError::Io { .. })
        ));
    })
    .await;
}

#[test]
fn canonical_aliases_and_concurrent_once_share_one_execution() {
    let files = Arc::new(Filesystem::new());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("module.rs");
    fs::write(&path, "").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    files
        .register_module(&path, move |_, (): ()| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(123u32)
        })
        .unwrap();
    let mut threads = Vec::new();
    for _ in 0..8 {
        let files = files.clone();
        let path = path.clone();
        threads.push(std::thread::spawn(move || {
            files.require_once::<u32, _>(path, ()).unwrap()
        }));
    }
    let values: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        values
            .iter()
            .filter(|r| matches!(r, RequiredFile::Value(123)))
            .count(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    #[cfg(unix)]
    {
        let alias = dir.path().join("alias.rs");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            files.require_once::<u32, _>(alias, ()).unwrap(),
            RequiredFile::AlreadyIncluded
        );
    }
}

fn module_http(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let path = App::base_path("compiled.rs")?;
        let value = File::get_require::<String, _>(path, "HTTP".to_owned())?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({"module":value}),
        )
    })
}
fn missing_module_http(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        File::get_require::<String, _>(App::base_path("missing.rs")?, ())?;
        unreachable!()
    })
}
struct ModuleCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for ModuleCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        _: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let loaded = (|| {
                let path = App::base_path("compiled.rs")?;
                File::require_once::<String, _>(path, "CLI".to_owned())
            })()
            .map_err(|e: FileError| openrust::console::ConsoleError::new(e.to_string()))?;
            output.stdout.push_str(match &loaded {
                RequiredFile::AlreadyIncluded => "already-included",
                RequiredFile::Value(s) => s,
            });
            Ok(0)
        })
    }
}
#[tokio::test]
async fn actual_http_and_artisan_share_compiled_registration_and_inclusion_but_not_missing_errors()
{
    use openrust::{foundation::configuration::Routing, support::facades::Route};
    let mut app = TestApp::create(|b| {
        b.with_commands(|artisan| {
            artisan.add_command(Box::new(ModuleCommand(
                openrust::console::CommandDefinition::new(clap::Command::new("file:module")),
            )))
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/module", module_http)?;
            Route::get("/module-error", missing_module_http)
        }))
    })
    .unwrap();
    fs::write(
        app.root().join("compiled.rs"),
        "// compile this function with the application",
    )
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let marker = app.root().join("module-ran.txt");
        File::register_module(
            App::base_path("compiled.rs").unwrap(),
            move |files, input: String| {
                files.put(&marker, &input)?;
                Ok(format!("compiled:{input}"))
            },
        )
        .unwrap();
    })
    .await;
    app.get("/module")
        .await
        .unwrap()
        .assert_ok()
        .assert_json_path("module", &serde_json::json!("compiled:HTTP"));
    assert_eq!(
        fs::read_to_string(app.root().join("module-ran.txt")).unwrap(),
        "HTTP"
    );
    app.artisan(["file:module"])
        .await
        .assert_successful()
        .expects_output("already-included");
    assert_eq!(
        fs::read_to_string(app.root().join("module-ran.txt")).unwrap(),
        "HTTP"
    );
    app.call(
        http::Request::get("/module-error")
            .header("Accept", "application/json")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
}

struct ModuleJob(Arc<AtomicUsize>);
impl openrust::contracts::queue::JobHandler for ModuleJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async {
            let path = App::base_path("worker-module.rs")
                .map_err(openrust::queue::QueueError::execution)?;
            let _: RequiredFile<String> = File::require_once(path, "worker-input".to_owned())
                .map_err(openrust::queue::QueueError::execution)?;
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}
async fn actual_worker(driver: &str, url: Option<String>, redis: Option<u16>) {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        database::DatabaseManager,
        queue::{DispatchOptions, Payload},
        support::facades::Queue,
    };
    let id = format!("{:016x}", fastrand::u64(..));
    let table = format!("file_module_jobs_{id}");
    let config = serde_json::json!({"app":{"env":"testing"},"database":{"default":"acceptance","connections":{"acceptance":{"url":url}},"redis":{"options":{"prefix":format!("file-module:{id}:")},"default":{"port":redis.unwrap_or(6379)}}},"queue":{"default":driver,"connections":{"database":{"driver":"database","table":table,"retry_after":30},"redis":{"driver":"redis","retry_after":30,"block_for":0}},"failed":{"driver":"null"}}});
    let mut app = TestApp::create(|b| {
        b.with_config(|root, _| {
            Ok(
                serde_json::from_slice(&fs::read(root.join("worker-config.json")).unwrap())
                    .unwrap(),
            )
        })
    })
    .unwrap();
    fs::write(app.root().join("worker-config.json"), config.to_string()).unwrap();
    fs::write(app.root().join("worker-module.rs"), "// 编译后的测试模块").unwrap();
    app.boot().await.unwrap();
    if driver == "database" {
        let db = app
            .app()
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap();
        db.get_schema_builder()
            .create(&table, |t| {
                t.id();
                t.string("queue", 255).index();
                t.long_text("payload");
                t.unsigned_integer("attempts");
                t.unsigned_big_integer("reserved_at").nullable();
                t.unsigned_big_integer("available_at");
                t.unsigned_big_integer("created_at");
            })
            .await
            .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    App::set_facade_application(Some(app.app()), async {
        let calls = calls.clone();
        let marker = app.root().join("worker-result.txt");
        File::register_module(
            App::base_path("worker-module.rs").unwrap(),
            move |files, input: String| {
                calls.fetch_add(1, Ordering::SeqCst);
                let bytes = files
                    .when(!input.is_empty(), |files, _| {
                        files.put(&marker, &input).map(Some)
                    })?
                    .into_value();
                assert_eq!(bytes, Some(input.len()));
                Ok(input)
            },
        )
        .unwrap();
        Queue::register("file-module", Arc::new(ModuleJob(completed.clone()))).unwrap();
        Queue::without_interruption_polling().unwrap();
        for _ in 0..2 {
            Queue::dispatch(
                Payload::new("file-module", ()).unwrap(),
                DispatchOptions {
                    queue: Some("isolated-module".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        }
    })
    .await;
    let result = app
        .artisan([
            "queue:work",
            driver,
            "--queue=isolated-module",
            "--stop-when-empty",
            "--sleep=0",
            "--timeout=0",
            "--tries=1",
        ])
        .await;
    result.assert_successful();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(completed.load(Ordering::SeqCst), 2);
    assert_eq!(
        fs::read_to_string(app.root().join("worker-result.txt")).unwrap(),
        "worker-input"
    );
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(
            File::require_once::<String, _>(
                App::base_path("worker-module.rs").unwrap(),
                "after-worker".to_owned()
            )
            .unwrap(),
            RequiredFile::AlreadyIncluded
        );
    })
    .await;
    if driver == "database" {
        app.app()
            .container()
            .make::<DatabaseManager>()
            .unwrap()
            .connection(None)
            .unwrap()
            .get_schema_builder()
            .drop_if_exists(&table)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn mysql_worker_loads_registered_module_once_across_real_jobs() {
    match std::env::var("OPENRUST_TEST_MYSQL_URL") {
        Ok(url) => actual_worker("database", Some(url), None).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "缺少MySQL验收夹具"
        ),
    }
}
#[tokio::test]
async fn redis_worker_loads_registered_module_once_across_real_jobs() {
    match std::env::var("OPENRUST_TEST_REDIS_PORT") {
        Ok(port) => actual_worker("redis", None, Some(port.parse().unwrap())).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none(),
            "缺少Redis验收夹具"
        ),
    }
}
