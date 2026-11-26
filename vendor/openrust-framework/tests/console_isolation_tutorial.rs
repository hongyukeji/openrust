use openrust::{
    cache::Ttl,
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{console::Isolatable, container::Container as _, foundation::Application as _},
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
    },
    support::facades::{App, Artisan, Cache, Facade, Route},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};
struct Signal {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Notify,
}
impl Default for Signal {
    fn default() -> Self {
        Self {
            entered: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Notify::new(),
        }
    }
}
struct Task(CommandDefinition);
impl Default for Task {
    fn default() -> Self {
        Self(
            CommandDefinition::from_signature(
                "tutorial:isolated {user} {--hold} {--fail} {--ttl=3600} {--absolute}",
            )
            .unwrap(),
        )
    }
}
impl Command for Task {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            if input.option("hold") == Some(json!(true)) {
                let signal = app
                    .make::<Signal>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                signal.entered.add_permits(1);
                signal.release.notified().await;
            }
            if input.option("fail") == Some(json!(true)) {
                return Err(ConsoleError::new("命令验收失败"));
            }
            output.comment(&format!(
                "ran:{}",
                input.argument("user").unwrap().as_str().unwrap()
            ));
            Ok(0)
        })
    }
}
impl Isolatable for Task {
    fn isolatable_id(&self, input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(input
            .argument("user")
            .and_then(|v| v.as_str().map(str::to_owned)))
    }
    fn isolation_lock_expires_at(&self, input: &Input) -> Result<Ttl, ConsoleError> {
        let seconds = input.input().integer("ttl");
        if input.option("absolute") == Some(json!(true)) {
            Ok(Ttl::Until(
                SystemTime::now() + Duration::from_secs(u64::try_from(seconds).unwrap_or(0)),
            ))
        } else {
            Ok(Ttl::Seconds(seconds))
        }
    }
}
struct Plain(CommandDefinition);
impl Default for Plain {
    fn default() -> Self {
        Self(CommandDefinition::from_signature("tutorial:plain {user}").unwrap())
    }
}
impl Command for Plain {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            output.comment(&format!("plain:{}", input.argument("user").unwrap()));
            Ok(0)
        })
    }
}
impl Isolatable for Plain {}
struct Legacy(CommandDefinition);
impl Default for Legacy {
    fn default() -> Self {
        Self(
            CommandDefinition::from_signature("tutorial:legacy-lock {user}")
                .unwrap()
                .isolatable(),
        )
    }
}
impl Command for Legacy {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async { Ok(0) })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add_isolatable(Box::<Task>::default())?;
    Artisan::add_isolatable(Box::<Plain>::default())?;
    Artisan::add(Box::<Legacy>::default())
}
fn load(
    root: &Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap())
}
fn configure(builder: ApplicationBuilder) -> ApplicationBuilder {
    builder
        .with_config(load)
        .with_routing(Routing::default().commands(commands))
}
async fn fixture() -> TestApp {
    let driver = std::env::var("ISOLATION_CACHE_DRIVER").unwrap_or_else(|_| "file".into());
    let database = std::env::var("ISOLATION_DATABASE_KIND").unwrap_or_else(|_| "sqlite".into());
    let mut fixture = if driver == "database" && database == "mysql" {
        TestApp::mysql(
            &std::env::var("OPENRUST_TEST_MYSQL_URL").expect("需要独占MySQL夹具"),
            configure,
        )
        .await
        .unwrap()
    } else if driver == "database" && database == "sqlite" {
        TestApp::sqlite(configure).unwrap()
    } else {
        TestApp::create(configure).unwrap()
    };
    let tag = fixture
        .root()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let settings = json!({"app":{"env":"testing","locale":"en"},"logging":{"default":"null","channels":{"null":{"driver":"null"}}},"cache":{"default":"tutorial","stores":{"tutorial":{"driver":driver,"path":"storage/cache/data","lock_path":"storage/cache/locks","table":format!("cache_{tag}"),"lock_table":format!("locks_{tag}"),"prefix":format!("isolation:{tag}:")}}}});
    let mut settings = settings.as_object().unwrap().clone();
    if driver == "redis" {
        settings.insert("database".into(),json!({"redis":{"default":{"url":std::env::var("ISOLATION_REDIS_URL").expect("需要独占Redis8 URL")}}}));
    }
    if driver == "database" && database == "postgres" {
        settings.insert("database".into(),json!({"default":"fixture","connections":{"fixture":{"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("需要独占PostgreSQL18 URL")}}}));
    }
    std::fs::write(
        fixture.root().join("fixture.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    fixture
        .app()
        .container()
        .instance(Arc::new(Signal::default()))
        .unwrap();
    fixture.boot().await.unwrap();
    if driver == "database" {
        let connection = fixture.database().await.unwrap();
        let mut schema = connection.get_schema_builder();
        schema
            .create(&format!("cache_{tag}"), |table| {
                table.string("key", None).primary();
                table.medium_text("value");
                table.integer("expiration");
            })
            .await
            .unwrap();
        schema
            .create(&format!("locks_{tag}"), |table| {
                table.string("key", None).primary();
                table.string("owner", None);
                table.integer("expiration");
            })
            .await
            .unwrap();
        settings.insert(
            "database".into(),
            fixture.app().config().unwrap().get("database").unwrap(),
        );
        std::fs::write(
            fixture.root().join("fixture.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
    }
    fixture
}
async fn owner(fixture: &TestApp) -> Arc<Application> {
    App::set_facade_application(Some(fixture.app()), async {
        App::get_facade_root().unwrap()
    })
    .await
}
async fn run(app: &Application, args: &[&str]) -> Result<(i32, Output), ConsoleError> {
    App::set_facade_application(Some(app), async {
        let mut output = Output::default();
        let args = args.iter().map(Into::into).collect::<Vec<_>>();
        let status = Artisan::call("tutorial:isolated", &args, &mut output).await?;
        Ok((status, output))
    })
    .await
}
async fn entered(signal: &Signal) {
    tokio::time::timeout(Duration::from_secs(5), signal.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget()
}
#[tokio::test]
async fn store_custom_ids_optional_switch_failures_cancellation_and_independent_artisan() {
    if let Ok(root) = std::env::var("ISOLATION_CHILD_ROOT") {
        let app = Application::configure(root.into())
            .with_config(load)
            .with_routing(Routing::default().commands(commands))
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["tutorial:isolated", "Ada", "--isolated=23"].map(Into::into))
                .await
                .unwrap(),
            23
        );
        return;
    }
    let fixture = fixture().await;
    let app = owner(&fixture).await;
    let signal = app.make::<Signal>().await.unwrap();
    App::set_facade_application(Some(app.as_ref()), async {
        for (command, key, user) in [
            (
                "tutorial:plain",
                "framework/command-tutorial:plain",
                "Grace",
            ),
            (
                "tutorial:legacy-lock",
                "framework/command-tutorial:legacy-lock",
                "Grace",
            ),
            (
                "tutorial:isolated",
                "framework/command-tutorial:isolated-",
                "",
            ),
        ] {
            let mut lock = Cache::lock(key, Some(Duration::from_secs(10))).unwrap();
            assert!(lock.get().await.unwrap());
            assert_eq!(
                Artisan::call(
                    command,
                    &[user.into(), "--isolated=17".into()],
                    &mut Output::default()
                )
                .await
                .unwrap(),
                17
            );
            assert!(lock.release().await.unwrap());
            assert_eq!(
                Artisan::call(
                    command,
                    &[user.into(), "--isolated=17".into()],
                    &mut Output::default()
                )
                .await
                .unwrap(),
                0
            );
        }
    })
    .await;
    let first = tokio::spawn({
        let app = app.clone();
        async move { run(&app, &["Ada", "--hold", "--isolated"]).await }
    });
    entered(&signal).await;
    assert_eq!(run(&app, &["Ada", "--isolated=17"]).await.unwrap().0, 17);
    assert_eq!(run(&app, &["Ada", "--isolated"]).await.unwrap().0, 0);
    assert_eq!(run(&app, &["Grace", "--isolated=17"]).await.unwrap().0, 0);
    assert_eq!(run(&app, &["Ada"]).await.unwrap().0, 0);
    assert!(run(&app, &["Grace", "--isolated", "--fail"]).await.is_err());
    assert_eq!(run(&app, &["Grace", "--isolated=17"]).await.unwrap().0, 0);
    if std::env::var("ISOLATION_CACHE_DRIVER").unwrap_or_else(|_| "file".into()) != "array" {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store_custom_ids_optional_switch_failures_cancellation_and_independent_artisan",
                "--nocapture",
            ])
            .env("ISOLATION_CHILD_ROOT", fixture.root())
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(
            String::from_utf8(child.stdout)
                .unwrap()
                .contains("already running")
        );
    }
    first.abort();
    assert!(matches!(first.await,Err(error) if error.is_cancelled()));
    let mut released = false;
    for _ in 0..100 {
        if run(&app, &["Ada", "--isolated=17"]).await.unwrap().0 == 0 {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(released, "取消后必须释放原锁");
    drop(app);
    fixture.close().await.unwrap();
}
#[tokio::test]
async fn store_relative_and_absolute_expiration_keep_the_replacement_owner() {
    let fixture = fixture().await;
    let app = owner(&fixture).await;
    let signal = app.make::<Signal>().await.unwrap();
    App::set_facade_application(Some(app.as_ref()), async {
        let mut lock = Cache::lock("refresh-probe", Some(Duration::from_secs(1))).unwrap();
        assert!(lock.get().await.unwrap());
        let before = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(lock.refresh(Some(Duration::from_secs(3))).await.unwrap());
        let after = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if std::env::var("ISOLATION_CACHE_DRIVER").ok().as_deref() == Some("database") {
            let table = app
                .config()
                .unwrap()
                .get("cache.stores.tutorial.lock_table")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            let connection = app
                .make::<openrust::database::DatabaseManager>()
                .await
                .unwrap()
                .connection(None)
                .unwrap();
            let row = connection.table(&table).first().await.unwrap().unwrap();
            let expiration = match row.get("expiration").unwrap() {
                openrust::database::Value::Integer(value) => u64::try_from(*value).unwrap(),
                openrust::database::Value::Unsigned(value) => *value,
                value => panic!("unexpected expiration {value:?}"),
            };
            assert!(
                (before + 3..=after + 3).contains(&expiration),
                "整秒SQL续租应为当前Unix秒加租期"
            );
        }
        assert!(lock.refresh(Some(Duration::from_secs(1))).await.unwrap());
        tokio::time::sleep(Duration::from_millis(1250)).await;
        let mut replacement = Cache::lock("refresh-probe", Some(Duration::from_secs(5))).unwrap();
        assert!(replacement.get().await.unwrap());
        assert!(!lock.release().await.unwrap());
        assert!(replacement.release().await.unwrap());
    })
    .await;
    for absolute in [false, true] {
        let first = tokio::spawn({
            let app = app.clone();
            async move {
                let mut args = vec!["Ada", "--hold", "--isolated", "--ttl=1"];
                if absolute {
                    args.push("--absolute")
                }
                run(&app, &args).await
            }
        });
        entered(&signal).await;
        tokio::time::sleep(Duration::from_millis(1250)).await;
        let second = tokio::spawn({
            let app = app.clone();
            async move { run(&app, &["Ada", "--hold", "--isolated", "--ttl=3600"]).await }
        });
        entered(&signal).await;
        signal.release.notify_one();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), first)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .0,
            0
        );
        assert_eq!(run(&app, &["Ada", "--isolated=17"]).await.unwrap().0, 17);
        signal.release.notify_one();
        assert_eq!(second.await.unwrap().unwrap().0, 0);
        assert_eq!(run(&app, &["Ada", "--isolated=17"]).await.unwrap().0, 0);
    }
    drop(app);
    fixture.close().await.unwrap();
}
#[tokio::test]
async fn console_application_and_kernel_contract_automatically_offer_isolation() {
    use openrust::contracts::console::Kernel as _;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("fixture.json"),
        serde_json::to_vec(
            &json!({"cache":{"default":"array","stores":{"array":{"driver":"array"}}}}),
        )
        .unwrap(),
    )
    .unwrap();
    let app = Application::configure(root.path().into())
        .with_config(load)
        .create()
        .unwrap();
    let mut console = openrust::console::Application::new();
    console
        .add_isolatable_command(Box::<Task>::default())
        .unwrap();
    let mut kernel = openrust::foundation::console::Kernel::new(app, console);
    kernel.bootstrap().await.unwrap();
    let mut initial = Output::default();
    assert_eq!(
        kernel
            .handle(
                &["tutorial:isolated", "Ada", "--isolated"].map(Into::into),
                &mut initial
            )
            .await,
        0
    );
    kernel
        .register_isolatable_command(Box::<Task>::default())
        .unwrap();
    let definition = kernel
        .all()
        .into_iter()
        .find(|v| v.name() == "tutorial:isolated")
        .unwrap();
    assert!(
        definition
            .parse(&["Ada".into(), "--isolated=13".into()])
            .is_ok()
    );
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(
                &["tutorial:isolated", "Ada", "--isolated"].map(Into::into),
                &mut output
            )
            .await,
        0
    );
    assert!(output.stdout.contains("ran:Ada"));
}

#[tokio::test]
async fn actual_http_observes_the_same_isolatable_command_lock() {
    use axum::handler::Handler as _;
    use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
    fn routes() -> Result<(), openrust::routing::RoutingError> {
        Route::get("/isolated", || async {
            let mut output = Output::default();
            Artisan::call(
                "tutorial:isolated",
                &["Ada".into(), "--isolated=17".into()],
                &mut output,
            )
            .await
            .unwrap()
            .to_string()
        })?;
        Route::get("/different", || async {
            let mut output = Output::default();
            Artisan::call(
                "tutorial:isolated",
                &["Grace".into(), "--isolated=17".into()],
                &mut output,
            )
            .await
            .unwrap()
            .to_string()
        })
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("fixture.json"),serde_json::to_vec(&json!({"app":{"env":"testing"},"cache":{"default":"file","stores":{"file":{"driver":"file","path":"data","lock_path":"locks"}}},"logging":{"default":"null","channels":{"null":{"driver":"null"}}}})).unwrap()).unwrap();
    let app = Application::configure(root.path().into())
        .with_config(load)
        .with_routing(Routing::default().commands(commands).web(routes))
        .create()
        .unwrap();
    app.container()
        .instance(Arc::new(Signal::default()))
        .unwrap();
    let handle =
        App::set_facade_application(Some(&app), async { App::get_facade_root().unwrap() }).await;
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let signal = handle.make::<Signal>().await.unwrap();
    let task = tokio::spawn({
        let app = handle.clone();
        async move { run(&app, &["Ada", "--hold", "--isolated"]).await }
    });
    entered(&signal).await;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    for (path, expected) in [("isolated", "17"), ("different", "0")] {
        let response = reqwest::get(format!("http://{address}/{path}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), expected);
    }
    signal.release.notify_one();
    assert_eq!(task.await.unwrap().unwrap().0, 0);
    assert_eq!(
        reqwest::get(format!("http://{address}/isolated"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "0"
    );
    server.abort();
    let _ = server.await;
}
