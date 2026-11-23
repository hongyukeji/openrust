use openrust::contracts::foundation::Application as _;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::debug::{Exception, ExceptionHandler, SharedException},
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogResult,
    support::facades::{App, Exceptions, Facade, Route},
    testing::{ExceptionType, TestApp},
};
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
struct Failure(Arc<Mutex<usize>>);
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failure {}", self.0.lock().unwrap())
    }
}
impl Error for Failure {}
#[derive(Debug)]
struct Wrapper(Failure);
impl fmt::Display for Wrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("wrapper")
    }
}
impl Error for Wrapper {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}
#[derive(Default)]
struct Observed(AtomicUsize);
impl ExceptionHandler for Observed {
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { true })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
        Response::new(
            StatusCode::IM_A_TEAPOT,
            openrust::http::Body::from("underlying"),
        )
    }
    fn render_for_console(&self, output: &mut Output, _: &Exception<'_>) {
        output.stderr.push_str("underlying\n");
    }
}
struct Fail(CommandDefinition);
impl Command for Fail {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async { Err(ConsoleError::new("fake-command")) })
    }
}
fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(std::io::Error::other("fake-http")),
        })
    })
}
async fn app() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_commands(|a| {
            a.add_command(Box::new(Fail(CommandDefinition::new(clap::Command::new(
                "fake:fail",
            )))))
        })
        .with_routing(Routing::default().web(|| Route::get("/failure", fail)))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn owned_non_clone_errors_keep_identity_and_predicates_can_reenter() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    let state = Arc::new(Mutex::new(1));
    App::set_facade_application(Some(app.app()), async {
        assert!(!Exceptions::is_fake().unwrap());
        Exceptions::report(Failure(state.clone())).await.unwrap();
        assert!(Exceptions::is_fake().unwrap());
        *state.lock().unwrap() = 9;
        Exceptions::assert_reported_where::<Failure>(|e| {
            fake.assert_reported_count(1);
            Arc::ptr_eq(&e.0, &state) && *e.0.lock().unwrap() == 9
        })
        .unwrap();
        Exceptions::assert_not_reported::<Wrapper>().unwrap();
        assert_eq!(Exceptions::reported().unwrap().len(), 1);
    })
    .await;
    fake.assert_reported::<Failure>();
    let failure = fake.reported();
    let error = Exception::Error(&failure[0]);
    assert!(Arc::ptr_eq(
        &error.downcast_ref::<Failure>().unwrap().0,
        &state
    ));
}
#[tokio::test]
async fn exact_root_selection_and_assertions_do_not_match_sources() {
    let mut app = app().await;
    let observed = Arc::new(Observed::default());
    app.instance::<dyn ExceptionHandler>(observed.clone())
        .await
        .unwrap();
    let fake = app
        .fake_exceptions(&[ExceptionType::of::<Failure>()])
        .await
        .unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::report(Wrapper(Failure(Arc::new(Mutex::new(1)))))
            .await
            .unwrap();
        fake.assert_nothing_reported();
        assert_eq!(observed.0.load(Ordering::SeqCst), 1);
        Exceptions::report(Failure(Arc::new(Mutex::new(2))))
            .await
            .unwrap();
        fake.assert_reported_count(1);
        let second =
            Exceptions::fake_with(vec![ExceptionType::including_sources::<Failure>()]).unwrap();
        assert!(Arc::ptr_eq(
            &Exceptions::handler().unwrap(),
            &(observed.clone() as Arc<dyn ExceptionHandler>)
        ));
        Exceptions::report(Wrapper(Failure(Arc::new(Mutex::new(3)))))
            .await
            .unwrap();
        second.assert_reported::<Wrapper>();
        second.assert_not_reported::<Failure>();
        fake.assert_reported_count(1);
    })
    .await;
}
#[tokio::test]
async fn ignored_errors_and_replacing_original_handler_preserve_records() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::dont_report::<Failure>().unwrap();
        Exceptions::report(Failure(Arc::new(Mutex::new(1))))
            .await
            .unwrap();
        fake.assert_nothing_reported();
        Exceptions::stop_ignoring::<Failure>().unwrap();
        Exceptions::report(Failure(Arc::new(Mutex::new(2))))
            .await
            .unwrap();
        let observed = Arc::new(Observed::default());
        Exceptions::set_handler(observed.clone()).unwrap();
        fake.assert_reported_count(1);
        assert!(Arc::ptr_eq(
            &Exceptions::handler().unwrap(),
            &(observed as Arc<dyn ExceptionHandler>)
        ));
    })
    .await;
}
#[tokio::test]
async fn throw_results_preserve_the_original_error_and_first_reported_order() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    fake.throw_first_reported().unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::throw_on_report().unwrap();
        let result = Exceptions::report(Failure(Arc::new(Mutex::new(7))))
            .await
            .unwrap_err();
        let error = Exception::Error(&result);
        assert_eq!(
            *error.downcast_ref::<Failure>().unwrap().0.lock().unwrap(),
            7
        );
        let first = Exceptions::throw_first_reported().err().unwrap();
        assert!(Exception::Error(&first).downcast_ref::<Failure>().is_some());
    })
    .await;
    fake.assert_reported_count(1);
}
#[tokio::test]
async fn actual_http_and_artisan_report_owned_errors_and_keep_rendering() {
    let mut first = app().await;
    let mut second = app().await;
    let fake = first.fake_exceptions(&[]).await.unwrap();
    first
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    fake.assert_reported::<HttpError>();
    fake.assert_reported_count(1);
    let command = first.artisan(["fake:fail"]).await;
    command.assert_exit_code(1);
    assert!(command.stderr.contains("fake-command"));
    fake.assert_reported::<ConsoleError>();
    fake.assert_reported_count(2);
    second
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    fake.assert_reported_count(2);
}
#[tokio::test]
async fn selective_http_delegates_unselected_and_renders_selected_without_double_reporting() {
    let mut app = app().await;
    let observed = Arc::new(Observed::default());
    app.instance::<dyn ExceptionHandler>(observed.clone())
        .await
        .unwrap();
    let fake = app
        .fake_exceptions(&[ExceptionType::of::<Failure>()])
        .await
        .unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
    fake.assert_nothing_reported();
    assert_eq!(observed.0.load(Ordering::SeqCst), 1);
    let fake = app.fake_exceptions(&[]).await.unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
    fake.assert_reported::<HttpError>();
    assert_eq!(observed.0.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn borrowed_opaque_errors_fail_explicitly_and_assertions_fail_on_mismatch() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    let error = Failure(Arc::new(Mutex::new(1)));
    assert!(fake.report(&Exception::Error(&error)).await.is_err());
    fake.assert_nothing_reported();
    let shared = SharedException::new(error);
    fake.report(&Exception::Error(&shared)).await.unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || fake.assert_not_reported::<Failure>()
        ))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || fake.assert_reported::<Wrapper>()
        ))
        .is_err()
    );
}

struct FailedJob;
impl openrust::contracts::queue::JobHandler for FailedJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async {
            Err(openrust::queue::QueueError::execution(Failure(Arc::new(
                Mutex::new(42),
            ))))
        })
    }
}
async fn worker_fake(driver: &str, url: Option<String>, redis: Option<u16>, rethrow: bool) {
    use openrust::{
        contracts::container::Container as _,
        database::DatabaseManager,
        queue::{DispatchOptions, Payload, QueueError},
        support::facades::Queue,
    };
    use serde_json::{Value, json};
    let id = format!("{:016x}", fastrand::u64(..));
    let table = format!("fake_jobs_{id}");
    let table_config = table.clone();
    let prefix = format!("exception-fake:{id}:");
    let selected = driver.to_owned();
    let config = json!({"app":{"env":"testing"},"database":{"default":"acceptance","connections":{"acceptance":{"url":url}},"redis":{"options":{"prefix":prefix},"default":{"port":redis.unwrap_or(6379)}}},"cache":{"default":"array","stores":{"array":{"driver":"array"}}},"queue":{"default":selected,"connections":{"database":{"driver":"database","table":table_config,"retry_after":30},"redis":{"driver":"redis","retry_after":30,"block_for":0}},"failed":{"driver":"null"}}});
    let mut app = TestApp::create(|b| {
        b.with_config(|root, _| {
            let contents = std::fs::read(root.join("fake-worker.json")).unwrap();
            Ok(serde_json::from_slice(&contents).unwrap())
        })
    })
    .unwrap();
    std::fs::write(app.root().join("fake-worker.json"), config.to_string()).unwrap();
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
    let fake = app.fake_exceptions(&[]).await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Queue::register("fake-failure", Arc::new(FailedJob)).unwrap();
        Queue::without_interruption_polling().unwrap();
        Queue::dispatch(
            Payload::new("fake-failure", Value::Null).unwrap(),
            DispatchOptions {
                queue: Some("isolated-fake".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    })
    .await;
    let args = [
        "queue:work",
        driver,
        "--queue=isolated-fake",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--tries=1",
    ];
    if rethrow {
        fake.throw_on_report();
        let error = app
            .try_artisan(args)
            .await
            .expect_err("Worker 必须传播原任务错误");
        let exception = Exception::Error(&error);
        assert!(exception.downcast_ref::<QueueError>().is_some());
        assert_eq!(
            *exception
                .downcast_ref::<Failure>()
                .unwrap()
                .0
                .lock()
                .unwrap(),
            42
        );
    } else {
        app.artisan(args).await.assert_exit_code(0);
    }
    fake.assert_reported::<QueueError>();
    fake.assert_reported_count(1);
    let reports = fake.reported();
    let exception = Exception::Error(&reports[0]);
    let failure = exception
        .downcast_ref::<Failure>()
        .expect("Worker 保留任务原始错误");
    assert_eq!(*failure.0.lock().unwrap(), 42);
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
async fn mysql_artisan_worker_records_original_job_error() {
    match std::env::var("OPENRUST_TEST_MYSQL_URL") {
        Ok(url) => worker_fake("database", Some(url), None, false).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "缺少 MySQL 验收夹具"
        ),
    }
}
#[tokio::test]
async fn redis_artisan_worker_records_original_job_error() {
    match std::env::var("OPENRUST_TEST_REDIS_PORT") {
        Ok(port) => worker_fake("redis", None, Some(port.parse().unwrap()), false).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none(),
            "缺少 Redis 验收夹具"
        ),
    }
}

#[tokio::test]
async fn throw_mode_interrupts_http_and_artisan_with_original_errors_and_restores_scopes() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    fake.throw_on_report();
    let error = match app.get("/failure").await {
        Err(error) => error,
        Ok(_) => panic!("HTTP 必须传播异常，不返回渲染后的成功调用"),
    };
    assert!(
        Exception::Error(error.as_ref())
            .downcast_ref::<HttpError>()
            .is_some()
    );
    let error = app
        .try_artisan(["fake:fail"])
        .await
        .expect_err("Artisan 必须传播异常");
    assert!(
        Exception::Error(&error)
            .downcast_ref::<ConsoleError>()
            .is_some()
    );
    fake.assert_reported_count(2);
    App::set_facade_application(Some(app.app()), async {
        // 受保护调用完成后不遗留通道；直接报告仍返回普通 Result 错误。
        let error = Exceptions::report(Failure(Arc::new(Mutex::new(8))))
            .await
            .unwrap_err();
        assert!(Exception::Error(&error).downcast_ref::<Failure>().is_some());
    })
    .await;
}
#[tokio::test]
async fn capture_cancels_after_report_and_nested_scopes_restore_without_cross_task_leaks() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::fake().unwrap().throw_on_report();
        let reached = AtomicUsize::new(0);
        let outer = Exceptions::catch_reported(async {
            let inner = Exceptions::catch_reported(async {
                Exceptions::report(Failure(Arc::new(Mutex::new(1))))
                    .await
                    .unwrap();
                reached.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap_err();
            assert_eq!(
                *Exception::Error(&inner)
                    .downcast_ref::<Failure>()
                    .unwrap()
                    .0
                    .lock()
                    .unwrap(),
                1
            );
            Exceptions::report(Failure(Arc::new(Mutex::new(2))))
                .await
                .unwrap();
            reached.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap_err();
        assert_eq!(
            *Exception::Error(&outer)
                .downcast_ref::<Failure>()
                .unwrap()
                .0
                .lock()
                .unwrap(),
            2
        );
        assert_eq!(reached.load(Ordering::SeqCst), 0);
    })
    .await;
    let (first, second) = tokio::join!(
        Exceptions::catch_reported(async { 42 }),
        Exceptions::catch_reported(async { 43 })
    );
    assert_eq!(first.unwrap(), 42);
    assert_eq!(second.unwrap(), 43);
}
#[tokio::test]
async fn cancelling_capture_drops_the_inflight_future_and_task_local_sender() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::fake().unwrap().throw_on_report();
        let timed = tokio::time::timeout(
            std::time::Duration::from_millis(5),
            Exceptions::catch_reported(std::future::pending::<()>()),
        )
        .await;
        assert!(timed.is_err());
        let result = Exceptions::report(Failure(Arc::new(Mutex::new(1)))).await;
        assert!(result.is_err());
    })
    .await;
}
#[tokio::test]
async fn infallible_artisan_compatibility_path_panics_only_for_explicit_throw_mode_with_original_payload()
 {
    use futures_util::FutureExt as _;
    let mut app = app().await;
    app.fake_exceptions(&[]).await.unwrap().throw_on_report();
    let payload = std::panic::AssertUnwindSafe(app.artisan(["fake:fail"]))
        .catch_unwind()
        .await
        .expect_err("显式重抛不得伪装成 TestCommand");
    let error = payload
        .downcast::<SharedException>()
        .expect("panic 必须保留原始共享错误");
    assert!(
        Exception::Error(error.as_ref())
            .downcast_ref::<ConsoleError>()
            .is_some()
    );
}

#[tokio::test]
async fn mysql_artisan_worker_propagates_original_job_error_in_throw_mode() {
    match std::env::var("OPENRUST_TEST_MYSQL_URL") {
        Ok(url) => worker_fake("database", Some(url), None, true).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none(),
            "缺少 MySQL 验收夹具"
        ),
    }
}
#[tokio::test]
async fn redis_artisan_worker_propagates_original_job_error_in_throw_mode() {
    match std::env::var("OPENRUST_TEST_REDIS_PORT") {
        Ok(port) => worker_fake("redis", None, Some(port.parse().unwrap()), true).await,
        Err(_) => assert!(
            std::env::var_os("OPENRUST_REQUIRE_QUEUE_REDIS").is_none(),
            "缺少 Redis 验收夹具"
        ),
    }
}

#[tokio::test]
async fn concurrent_http_calls_keep_throw_signals_and_report_lists_application_local() {
    let mut first = app().await;
    let mut second = app().await;
    let first_fake = first.fake_exceptions(&[]).await.unwrap();
    first_fake.throw_on_report();
    let second_fake = second.fake_exceptions(&[]).await.unwrap();
    let (left, right) = tokio::join!(first.get("/failure"), second.get("/failure"));
    assert!(left.is_err());
    right
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    first_fake.assert_reported_count(1);
    second_fake.assert_reported_count(1);
}
