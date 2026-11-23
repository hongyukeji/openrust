use openrust::contracts::foundation::Application as _;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::debug::{Exception, ExceptionHandler},
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogResult,
    support::facades::{App, Exceptions, Facade, Route},
    testing::TestApp,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct ObservedHandler {
    reports: AtomicUsize,
    renders: AtomicUsize,
    console: AtomicUsize,
}
impl ExceptionHandler for ObservedHandler {
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { true })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async {
            self.reports.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
        self.renders.fetch_add(1, Ordering::SeqCst);
        Response::json(StatusCode::IM_A_TEAPOT, "contract-handler").unwrap()
    }
    fn render_for_console(&self, output: &mut Output, _: &Exception<'_>) {
        self.console.fetch_add(1, Ordering::SeqCst);
        output.stderr.push_str("contract-console\n");
    }
    fn should_stop_retries(&self, _: &Exception<'_>) -> bool {
        true
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
        Box::pin(async { Err(ConsoleError::new("failed-command")) })
    }
}
fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(std::io::Error::other("failed-http")),
        })
    })
}
async fn fixture() -> TestApp {
    let mut app = TestApp::create(|builder| {
        builder
            .with_commands(|artisan| {
                artisan.add_command(Box::new(Fail(CommandDefinition::new(clap::Command::new(
                    "exceptions:fail",
                )))))
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/failure", fail)?;
                Route::get("/replace-in-request", swap_and_fail)
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn replacement_contract_is_used_by_public_facade_http_application_and_console() {
    let mut app = fixture().await;
    let observed = Arc::new(ObservedHandler::default());
    App::set_facade_application(Some(app.app()), async {
        Exceptions::swap(observed.clone()).unwrap();
        Exceptions::report(std::io::Error::other("public-report"))
            .await
            .unwrap();
        app.app()
            .report(&std::io::Error::other("application-report"))
            .await
            .unwrap();
        let error = std::io::Error::other("query");
        let exception = Exception::Error(&error);
        assert!(Exceptions::should_report(&exception).await.unwrap());
        assert!(Exceptions::should_stop_retries(&exception).unwrap());
        assert!(Arc::ptr_eq(
            &Exceptions::handler().unwrap(),
            &(observed.clone() as Arc<dyn ExceptionHandler>)
        ));
    })
    .await;
    assert_eq!(observed.reports.load(Ordering::SeqCst), 2);
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT)
        .assert_content("\"contract-handler\"");
    let result = app.artisan(["exceptions:fail"]).await;
    result.assert_exit_code(1);
    assert!(result.stderr.contains("contract-console"));
    assert_eq!(observed.reports.load(Ordering::SeqCst), 4);
    assert_eq!(observed.renders.load(Ordering::SeqCst), 1);
    assert_eq!(observed.console.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn handler_replacement_is_isolated_and_missing_bindings_do_not_use_a_stale_concrete_handler()
{
    let mut first = fixture().await;
    let mut second = fixture().await;
    let observed = Arc::new(ObservedHandler::default());
    first
        .instance::<dyn ExceptionHandler>(observed.clone())
        .await
        .unwrap();
    first
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
    second
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    use openrust::contracts::container::Container as _;
    first
        .app()
        .container()
        .forget_instance(openrust::container::ServiceKey::typed::<dyn ExceptionHandler>());
    first
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(observed.renders.load(Ordering::SeqCst), 1);
}

fn swap_and_fail(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Exceptions::swap(Arc::new(ObservedHandler::default()))?;
        fail(request).await
    })
}
#[tokio::test]
async fn replacing_the_handler_inside_the_request_takes_effect_at_the_error_boundary() {
    let mut app = fixture().await;
    app.get("/replace-in-request")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
}

#[derive(Debug)]
struct DomainFailure(&'static str);
impl std::fmt::Display for DomainFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for DomainFailure {}
#[tokio::test]
async fn runtime_rules_change_reporting_and_retries_without_replacing_the_handler() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let error = DomainFailure("visible");
        let exception = Exception::Error(&error);
        assert!(Exceptions::should_report(&exception).await.unwrap());
        Exceptions::dont_report::<DomainFailure>().unwrap();
        assert!(!Exceptions::should_report(&exception).await.unwrap());
        Exceptions::stop_ignoring::<DomainFailure>().unwrap();
        assert!(Exceptions::should_report(&exception).await.unwrap());
        Exceptions::ignore::<DomainFailure>().unwrap();
        assert!(!Exceptions::should_report(&exception).await.unwrap());
        Exceptions::stop_ignoring::<DomainFailure>().unwrap();
        Exceptions::dont_report_when(|exception| {
            exception
                .downcast_ref::<DomainFailure>()
                .is_some_and(|error| error.0 == "hidden")
        })
        .unwrap();
        assert!(
            !Exceptions::should_report(&Exception::Error(&DomainFailure("hidden")))
                .await
                .unwrap()
        );
        Exceptions::dont_retry::<DomainFailure>().unwrap();
        assert!(Exceptions::should_stop_retries(&exception).unwrap());
        Exceptions::dont_retry_when(|exception| {
            exception.downcast_ref::<std::io::Error>().is_some()
        })
        .unwrap();
        assert!(
            Exceptions::should_stop_retries(&Exception::Error(&std::io::Error::other("retry")))
                .unwrap()
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        Exceptions::reportable::<DomainFailure>(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            openrust::foundation::configuration::ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(error).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Exceptions::dont_flash(["pin", "profile.secret"]).unwrap();
        Exceptions::dont_report_duplicates().unwrap();
    })
    .await;
}
#[tokio::test]
async fn runtime_renderers_and_response_hooks_are_used_by_real_http() {
    let mut app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::renderable::<std::io::Error>(|_, _| {
            Some(Response::json(StatusCode::CONFLICT, "runtime-renderer").unwrap())
        })
        .unwrap();
        Exceptions::respond_using(|response, _, _| {
            response.header(
                http::HeaderName::from_static("x-runtime"),
                http::HeaderValue::from_static("configured"),
            )
        })
        .unwrap();
    })
    .await;
    let response = app.get("/failure").await.unwrap();
    response
        .assert_status(StatusCode::CONFLICT)
        .assert_content("\"runtime-renderer\"");
    assert_eq!(response.headers().get("x-runtime").unwrap(), "configured");
}
#[tokio::test]
async fn runtime_json_policy_and_log_context_change_the_existing_handler() {
    let mut app=TestApp::create(|builder|builder.with_config(|_,_|Ok([("app".into(),serde_json::json!({"env":"testing"})),("logging".into(),serde_json::json!({"default":"single","channels":{"single":{"driver":"single","path":"storage/logs/runtime.log","level":"debug"}}}))].into())).with_routing(Routing::default().web(||Route::get("/failure",fail)))).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::level::<DomainFailure>(openrust::log::Level::Warning).unwrap();
        Exceptions::build_context_using(|_| {
            [("runtime".into(), serde_json::json!("configured"))]
                .into_iter()
                .collect()
        })
        .unwrap();
        Exceptions::report(DomainFailure("log-entry"))
            .await
            .unwrap();
        Exceptions::should_render_json_when(|_, _| true).unwrap();
    })
    .await;
    let log =
        std::fs::read_to_string(app.app().base_path().join("storage/logs/runtime.log")).unwrap();
    assert!(log.contains("WARNING"));
    assert!(log.contains("configured"));
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR)
        .assert_json_path("message", &serde_json::json!("Server Error"));
}
#[tokio::test]
async fn reporting_queries_do_not_consume_throttle_budget_and_unsupported_handlers_error() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let admissions = Arc::new(AtomicUsize::new(0));
        let observed = admissions.clone();
        Exceptions::throttle_using::<DomainFailure>(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Some(
                openrust::foundation::configuration::ReportThrottle::Lottery(
                    openrust::support::Lottery::odds(0, 1).unwrap(),
                ),
            )
        })
        .unwrap();
        let error = DomainFailure("sampled");
        let exception = Exception::Error(&error);
        assert!(!Exceptions::is_reporting(&exception).unwrap());
        assert!(Exceptions::should_report(&exception).await.unwrap());
        assert!(Exceptions::should_report(&exception).await.unwrap());
        assert_eq!(admissions.load(Ordering::SeqCst), 0);
        Exceptions::report(DomainFailure("sampled-at-report"))
            .await
            .unwrap();
        assert_eq!(admissions.load(Ordering::SeqCst), 1);
        Exceptions::swap(Arc::new(ObservedHandler::default())).unwrap();
        assert!(Exceptions::is_reporting(&exception).is_err());
        assert!(Exceptions::dont_report::<DomainFailure>().is_err());
    })
    .await;
}

#[tokio::test]
async fn report_callbacks_can_register_new_rules_without_holding_the_configuration_lock() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::reportable::<DomainFailure>(|_| {
            Exceptions::dont_report::<std::io::Error>().unwrap();
            openrust::foundation::configuration::ReportDecision::Stop
        })
        .unwrap();
        Exceptions::report(DomainFailure("reentrant"))
            .await
            .unwrap();
        assert!(
            !Exceptions::should_report(&Exception::Error(&std::io::Error::other("hidden")))
                .await
                .unwrap()
        );
    })
    .await;
}

#[tokio::test]
async fn is_reporting_tracks_the_same_error_only_while_its_callback_runs() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        Exceptions::reportable::<DomainFailure>(move |error| {
            assert!(Exceptions::is_reporting(&Exception::Error(error)).unwrap());
            assert!(!Exceptions::is_reporting(&Exception::Error(&DomainFailure("other"))).unwrap());
            observed.fetch_add(1, Ordering::SeqCst);
            openrust::foundation::configuration::ReportDecision::Stop
        })
        .unwrap();
        assert!(!Exceptions::is_reporting(&Exception::Error(&DomainFailure("outside"))).unwrap());
        Exceptions::report(DomainFailure("inside")).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!Exceptions::is_reporting(&Exception::Error(&DomainFailure("after"))).unwrap());
    })
    .await;
}

#[tokio::test]
async fn shared_error_identity_is_retained_and_reporting_scope_restores_after_panic() {
    use futures_util::FutureExt as _;
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let seen = Arc::new(AtomicUsize::new(0));
        let observed = seen.clone();
        Exceptions::reportable::<DomainFailure>(move |error| {
            assert!(Exceptions::is_reporting(&Exception::Error(error)).unwrap());
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("模拟报告回调中断");
            }
            openrust::foundation::configuration::ReportDecision::Stop
        })
        .unwrap();
        let shared = openrust::contracts::debug::SharedException::new(DomainFailure("shared"));
        assert!(
            std::panic::AssertUnwindSafe(Exceptions::report(shared.clone()))
                .catch_unwind()
                .await
                .is_err()
        );
        assert!(!Exceptions::is_reporting(&Exception::Error(&shared)).unwrap());
        Exceptions::report(shared.clone()).await.unwrap();
        assert!(!Exceptions::is_reporting(&Exception::Error(&shared)).unwrap());
        Exceptions::dont_report_duplicates().unwrap();
        Exceptions::report(shared.clone()).await.unwrap();
        Exceptions::report(shared).await.unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 3);
    })
    .await;
}
