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
