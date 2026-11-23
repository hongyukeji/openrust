use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::debug::{Exception, ExceptionOwnershipError, SharedException},
    foundation::{Application, configuration::Routing},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{App, Exceptions, Facade, Route},
    testing::{ExceptionType, TestApp},
};
use std::{
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};
#[derive(Debug)]
struct Opaque(Mutex<u32>);
impl fmt::Display for Opaque {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opaque {}", self.0.lock().unwrap())
    }
}
impl Error for Opaque {}
fn fail(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(Opaque(Mutex::new(7))),
        })
    })
}
fn invalid(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let mut errors = openrust::support::MessageBag::default();
        errors.add("name", "required");
        Err(HttpError::Validation(Box::new(
            openrust::validation::ValidationException::new(errors),
        )))
    })
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
        Box::pin(async { Err(ConsoleError::new("without-command")) })
    }
}
async fn app() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_commands(|a| {
            a.add_command(Box::new(Fail(CommandDefinition::new(clap::Command::new(
                "without:fail",
            )))))
        })
        .with_exceptions(|e| {
            e.should_render_json_when(|_, _| true);
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/failure", fail)?;
            Route::get("/invalid", invalid)
        }))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn disabling_and_restoring_http_handling_preserves_non_clone_errors() {
    let mut app = app().await;
    app.without_exception_handling(&[]).await.unwrap();
    let error = app
        .get("/failure")
        .await
        .expect_err("关闭处理后不可渲染500");
    assert_eq!(
        *Exception::Error(error.as_ref())
            .downcast_ref::<Opaque>()
            .unwrap()
            .0
            .lock()
            .unwrap(),
        7
    );
    app.with_exception_handling().await.unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    app.without_exception_handling(&[]).await.unwrap();
    assert!(app.get("/failure").await.is_err());
}
#[tokio::test]
async fn not_found_adds_method_and_query_free_url_and_keeps_the_original_error() {
    let mut app = app().await;
    app.without_exception_handling(&[]).await.unwrap();
    let error = app
        .get("/missing?token=private")
        .await
        .expect_err("404应当重抛");
    assert_eq!(error.to_string(), "GET http://localhost/missing");
    assert!(matches!(
        Exception::Error(error.as_ref()).downcast_ref::<HttpError>(),
        Some(HttpError::NotFound)
    ));
}
#[tokio::test]
async fn enum_predicate_exceptions_render_only_selected_variants() {
    let mut app = app().await;
    app.handle_exceptions(&[ExceptionType::matching::<HttpError>(|error| {
        matches!(error, HttpError::NotFound)
    })])
    .await
    .unwrap();
    app.get("/missing")
        .await
        .unwrap()
        .assert_status(StatusCode::NOT_FOUND);
    assert!(app.get("/failure").await.is_err());
}
#[tokio::test]
async fn validation_only_handling_keeps_real_json_response_and_throws_other_errors() {
    let mut app = app().await;
    app.handle_validation_exceptions().await.unwrap();
    let response = app
        .call(
            http::Request::get("/invalid")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(app.get("/missing").await.is_err());
}
#[tokio::test]
async fn fake_survives_mode_changes_records_ignored_types_and_restores_original_eligibility() {
    let mut app = app().await;
    let fake = app.fake_exceptions(&[]).await.unwrap();
    app.get("/failure").await.unwrap();
    fake.assert_reported_count(1);
    app.without_exception_handling(&[]).await.unwrap();
    assert!(app.get("/missing").await.is_err());
    fake.assert_reported_count(2);
    fake.assert_reported_where::<HttpError>(|error| matches!(error, HttpError::NotFound));
    app.with_exception_handling().await.unwrap();
    app.get("/missing")
        .await
        .unwrap()
        .assert_status(StatusCode::NOT_FOUND);
    fake.assert_reported_count(2);
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    fake.assert_reported_count(3);
}
#[tokio::test]
async fn fake_installed_after_disabling_handler_still_records_and_rethrows() {
    let mut app = app().await;
    app.without_exception_handling(&[]).await.unwrap();
    let fake = app.fake_exceptions(&[]).await.unwrap();
    assert!(app.get("/missing").await.is_err());
    fake.assert_reported_count(1);
    app.with_exception_handling().await.unwrap();
    app.get("/missing").await.unwrap();
    fake.assert_reported_count(1);
}
#[tokio::test]
async fn public_render_returns_original_owned_error_and_borrowed_errors_fail_with_a_distinct_type()
{
    let mut app = app().await;
    app.without_exception_handling(&[]).await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let request = Request::from_parts(
            http::Request::get("http://localhost/direct")
                .body(())
                .unwrap()
                .into_parts()
                .0,
            Body::empty(),
        );
        let original = SharedException::new(Opaque(Mutex::new(9)));
        let expected = Exception::Error(&original)
            .downcast_ref::<Opaque>()
            .unwrap() as *const Opaque;
        let error = match Exceptions::render(&request, &Exception::Error(&original)) {
            Err(error) => error,
            Ok(_) => panic!("关闭处理应返回原始错误"),
        };
        let actual = Exception::Error(error.as_ref());
        assert_eq!(
            actual.downcast_ref::<Opaque>().unwrap() as *const Opaque,
            expected
        );
        let missing = SharedException::new(HttpError::NotFound);
        let expected_missing = Exception::Error(&missing)
            .downcast_ref::<HttpError>()
            .unwrap() as *const HttpError;
        let error = match Exceptions::render(&request, &Exception::Error(&missing)) {
            Err(error) => error,
            Ok(_) => panic!("404应当重抛"),
        };
        assert_eq!(error.to_string(), "GET http://localhost/direct");
        assert_eq!(
            Exception::Error(error.as_ref())
                .downcast_ref::<HttpError>()
                .unwrap() as *const HttpError,
            expected_missing
        );
        let borrowed = std::io::Error::other("borrowed");
        let error = match Exceptions::render(&request, &Exception::Error(&borrowed)) {
            Err(error) => error,
            Ok(_) => panic!("借用错误应明确拒绝"),
        };
        assert!(
            Exception::Error(error.as_ref())
                .downcast_ref::<ExceptionOwnershipError>()
                .is_some()
        );
        assert!(
            Exception::Error(error.as_ref())
                .downcast_ref::<std::io::Error>()
                .is_none()
        );
    })
    .await;
}
#[tokio::test]
async fn console_rendering_and_other_applications_keep_normal_behavior() {
    let mut first = app().await;
    let mut second = app().await;
    first.without_exception_handling(&[]).await.unwrap();
    let command = first.artisan(["without:fail"]).await;
    command.assert_exit_code(1);
    assert!(command.stderr.contains("without-command"));
    second
        .get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(first.get("/failure").await.is_err());
}

#[derive(Default)]
struct Observed {
    reports: std::sync::atomic::AtomicUsize,
    renders: std::sync::atomic::AtomicUsize,
}
impl openrust::contracts::debug::ExceptionHandler for Observed {
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { true })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, openrust::log::LogResult<()>> {
        Box::pin(async {
            self.reports
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    }
    fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
        self.renders
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Response::new(StatusCode::IM_A_TEAPOT, Body::from("custom-render"))
    }
    fn render_for_console(&self, _: &mut Output, _: &Exception<'_>) {}
}
#[tokio::test]
async fn excepted_errors_only_render_on_the_actual_original_handler_without_reporting_twice() {
    let mut app = app().await;
    let observed = Arc::new(Observed::default());
    app.instance::<dyn openrust::contracts::debug::ExceptionHandler>(observed.clone())
        .await
        .unwrap();
    app.without_exception_handling(&[ExceptionType::matching::<HttpError>(|e| {
        matches!(e, HttpError::Internal { .. })
    })])
    .await
    .unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
    assert_eq!(
        observed.reports.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    app.with_exception_handling().await.unwrap();
    app.get("/failure")
        .await
        .unwrap()
        .assert_status(StatusCode::IM_A_TEAPOT);
    assert_eq!(
        observed.reports.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        observed.renders.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn repeated_disable_keeps_the_first_original_handler_instead_of_nesting_wrappers() {
    let mut app = app().await;
    app.without_exception_handling(&[]).await.unwrap();
    app.without_exception_handling(&[ExceptionType::matching::<HttpError>(|error| {
        matches!(error, HttpError::NotFound)
    })])
    .await
    .unwrap();
    app.get("/missing")
        .await
        .unwrap()
        .assert_status(StatusCode::NOT_FOUND);
    assert!(app.get("/failure").await.is_err());
    app.without_exception_handling(&[]).await.unwrap();
    assert!(app.get("/missing").await.is_err());
    app.with_exception_handling().await.unwrap();
    app.get("/missing")
        .await
        .unwrap()
        .assert_status(StatusCode::NOT_FOUND);
}

#[derive(Debug)]
struct Cycle;
impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cycle")
    }
}
impl Error for Cycle {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self)
    }
}
#[tokio::test]
async fn explicit_source_selectors_inspect_mapped_diagnostics_and_cyclic_sources_are_bounded() {
    #[derive(Debug)]
    struct Wrapper(std::io::Error);
    impl fmt::Display for Wrapper {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("wrapped")
        }
    }
    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }
    let mut app = app().await;
    let fake = app
        .fake_exceptions(&[ExceptionType::including_sources::<std::io::Error>()])
        .await
        .unwrap();
    let owned = SharedException::new(Wrapper(std::io::Error::other("source")));
    let exception = Exception::Mapped(&owned);
    assert!(exception.downcast_ref::<std::io::Error>().is_none());
    assert!(exception.downcast_chain_ref::<std::io::Error>().is_some());
    use openrust::contracts::debug::ExceptionHandler as _;
    fake.report(&exception).await.unwrap();
    fake.assert_reported::<Wrapper>();
    fake.assert_not_reported::<std::io::Error>();
    let cycle = Cycle;
    assert!(
        Exception::Error(&cycle)
            .downcast_chain_ref::<std::io::Error>()
            .is_none()
    );
    assert!(
        Exception::Error(&cycle)
            .downcast_ref::<std::io::Error>()
            .is_none()
    );
}

#[tokio::test]
async fn shared_constructors_and_mapping_keep_duplicate_identity_instead_of_creating_wrapper_instances()
 {
    use openrust::foundation::configuration::ReportDecision;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::dont_report_duplicates().unwrap();
        Exceptions::reportable::<Opaque>(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            ReportDecision::Stop
        })
        .unwrap();
        let first = SharedException::new(Opaque(Mutex::new(1)));
        let again = SharedException::new(first.clone());
        let from_arc = SharedException::from_arc(Arc::new(first.clone()));
        Exceptions::report(first).await.unwrap();
        Exceptions::report(again).await.unwrap();
        Exceptions::report(from_arc).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mapped = SharedException::new(Opaque(Mutex::new(2)));
        Exceptions::map::<std::io::Error, SharedException>(move |_| mapped.clone()).unwrap();
        Exceptions::report(std::io::Error::other("first-mapping"))
            .await
            .unwrap();
        Exceptions::report(std::io::Error::other("same-target"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    })
    .await;
}
