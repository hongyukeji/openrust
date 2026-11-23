use openrust::support::facades::Facade as _;
use openrust::{
    contracts::debug::{Exception, ExceptionHandler, SharedException},
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response},
    log::{LogContext, LogResult},
    support::facades::{App, Auth, Exceptions, Route},
    testing::TestApp,
};
use serde_json::json;
use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Debug)]
struct Failure;
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("context-failure")
    }
}
impl Error for Failure {}
async fn app() -> TestApp {
    let mut app=TestApp::create(|b|b.with_config(|_,_|Ok([
 ("app".into(),json!({"env":"testing"})),
 ("logging".into(),json!({"default":"single","channels":{"single":{"driver":"single","path":"context.jsonl","formatter":"json"}}})),
 ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"cached"}}})),
 ].into())).with_routing(Routing::default().web(||{Route::get("/context/{id}",context_route)?;Route::get("/lazy",lazy_context_route)?;Route::get("/loaned",loaned_context_route)?;Route::get("/report-lazy",report_lazy_route)?;Route::get("/lazy-failure",lazy_failure_route)}))).unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn contextual_callbacks_receive_accumulated_fields_and_keep_original_error() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::register().unwrap();
        Exceptions::exception_context::<Failure>(|_| {
            LogContext::from_iter([
                ("stage".into(), json!(1)),
                ("origin".into(), json!("typed")),
            ])
        })
        .unwrap();
        Exceptions::build_context_using_with_context(|_, previous| {
            assert_eq!(previous["stage"], 1);
            LogContext::from_iter([("stage".into(), json!(2))])
        })
        .unwrap();
        Exceptions::build_context_using_with_context(|_, previous| {
            assert_eq!(previous["stage"], 2);
            LogContext::from_iter([("done".into(), json!(true))])
        })
        .unwrap();
        let shared = SharedException::new(Failure);
        let exception = Exception::Error(&shared);
        let context = Exceptions::build_context_for_exception(&exception).unwrap();
        assert_eq!(context["stage"], 2);
        assert_eq!(context["origin"], "typed");
        assert_eq!(context["done"], true);
        let full = Exceptions::context_for_exception(&exception).await.unwrap();
        assert_eq!(full.fields, context);
        assert!(std::ptr::eq(
            full.exception.downcast_ref::<Failure>().unwrap(),
            exception.downcast_ref::<Failure>().unwrap()
        ));
        Exceptions::report(shared).await.unwrap();
    })
    .await;
    let log = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(record["context"]["stage"], 2);
    assert_eq!(record["context"]["origin"], "typed");
}
#[tokio::test]
async fn callbacks_can_register_future_context_without_holding_configuration_locks() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Exceptions::build_context_using_with_context(|_, _| {
            Exceptions::exception_context::<std::io::Error>(|_| {
                LogContext::from_iter([("registered".into(), json!(true))])
            })
            .unwrap();
            LogContext::new()
        })
        .unwrap();
        let error = std::io::Error::other("first");
        let exception = Exception::Error(&error);
        assert!(
            !Exceptions::build_context_for_exception(&exception)
                .unwrap()
                .contains_key("registered")
        );
        assert_eq!(
            Exceptions::build_context_for_exception(&exception).unwrap()["registered"],
            true
        );
    })
    .await;
}
struct Original(AtomicUsize);
impl ExceptionHandler for Original {
    fn register(&self) -> Result<(), openrust::contracts::container::BindingResolutionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn build_context_for_exception(
        &self,
        _: &Exception<'_>,
    ) -> Result<LogContext, openrust::contracts::container::BindingResolutionError> {
        Ok(LogContext::from_iter([("custom".into(), json!(true))]))
    }
    fn context_for_exception<'a, 'e>(
        &'a self,
        exception: &'a Exception<'e>,
    ) -> HttpFuture<
        'a,
        Result<
            openrust::contracts::debug::ExceptionContext<'e>,
            openrust::contracts::container::BindingResolutionError,
        >,
    > {
        Box::pin(async move {
            Ok(openrust::contracts::debug::ExceptionContext {
                fields: self.build_context_for_exception(exception)?,
                exception: *exception,
            })
        })
    }
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { true })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
        Response::from("custom")
    }
    fn render_for_console(&self, _: &mut openrust::console::Output, _: &Exception<'_>) {}
}
#[tokio::test]
async fn register_and_context_delegate_to_custom_handler_through_fake() {
    let mut app = app().await;
    let handler = Arc::new(Original(AtomicUsize::new(0)));
    app.instance::<dyn ExceptionHandler>(handler.clone())
        .await
        .unwrap();
    app.fake_exceptions(&[]).await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Exceptions::register().unwrap();
        let error = Failure;
        let exception = Exception::Error(&error);
        assert_eq!(
            Exceptions::context_for_exception(&exception)
                .await
                .unwrap()
                .fields["custom"],
            true
        );
        assert_eq!(
            Exceptions::build_context_for_exception(&exception).unwrap()["custom"],
            true
        );
    })
    .await;
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}
struct Account(String);
impl openrust::contracts::auth::Authenticatable for Account {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        self.0.clone().into()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
fn context_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::auth::Factory as _;
        let id = request.route("id").unwrap().to_owned();
        Auth::manager()
            .unwrap()
            .guard(request, None)
            .unwrap()
            .set_user(Box::new(Account(id.clone())));
        let error = Failure;
        let exception = Exception::Error(&error);
        let full = Exceptions::context_for_exception(&exception).await.unwrap();
        if id == "0" {
            assert!(!full.fields.contains_key("userId"));
        } else {
            assert_eq!(full.fields["userId"], id);
        }
        assert!(
            !Exceptions::build_context_for_exception(&exception)
                .unwrap()
                .contains_key("userId")
        );
        Exceptions::report(Failure)
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        Ok(Response::from("context-ok"))
    })
}
#[tokio::test]
async fn authenticated_context_is_request_local_and_php_false_identifiers_are_omitted() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Auth::manager()
            .unwrap()
            .via_request("cached", |_| panic!("读取已解析身份不得触发认证"));
    })
    .await;
    app.get("/context/member-a").await.unwrap().assert_ok();
    app.get("/context/0").await.unwrap().assert_ok();
    let log = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let entries = log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(entries[0]["context"]["userId"], "member-a");
    assert!(entries[1]["context"].get("userId").is_none());
    App::set_facade_application(Some(app.app()), async {
        assert!(
            !Exceptions::context_for_exception(&Exception::Error(&Failure))
                .await
                .unwrap()
                .fields
                .contains_key("userId")
        );
    })
    .await;
}

#[tokio::test]
async fn shared_transport_is_not_logged_as_an_extra_exception_or_context_callback() {
    let app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    App::set_facade_application(Some(app.app()), async {
        let observed = calls.clone();
        Exceptions::exception_context::<Failure>(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            LogContext::from_iter([("once".into(), json!(true))])
        })
        .unwrap();
        Exceptions::report(SharedException::new(SharedException::new(Failure)))
            .await
            .unwrap();
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let log = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(record["context"]["once"], true);
    assert_eq!(
        record["context"]["exception"]["class"],
        std::any::type_name::<Failure>()
    );
    assert!(record["context"]["exception"].get("previous").is_none());
}

fn lazy_failure_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Err(HttpError::Internal {
            source: Box::new(Failure),
        })
    })
}
fn lazy_context_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let error = Failure;
        let exception = Exception::Error(&error);
        let context = Exceptions::context_for_exception_with_request(request, &exception).await?;
        assert_eq!(context.fields["userId"], "lazy-user");
        Ok(Response::from("lazy-context"))
    })
}
fn loaned_context_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request
            .with_container(async {
                let error = Failure;
                let context = Exceptions::context_for_exception(&Exception::Error(&error)).await?;
                assert_eq!(context.fields["userId"], "lazy-user");
                Ok(Response::from("loaned-context"))
            })
            .await
    })
}
async fn lazy_auth(app: &TestApp, calls: Arc<AtomicUsize>) {
    App::set_facade_application(Some(app.app()), async {
        Auth::manager().unwrap().via_request("cached", move |_| {
            let calls = calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(Box::new(Account("lazy-user".into()))
                    as Box<dyn openrust::contracts::auth::Authenticatable>))
            })
        });
    })
    .await;
}
#[tokio::test]
async fn http_error_reporting_resolves_default_guard_lazily_before_logging() {
    let mut app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    lazy_auth(&app, calls.clone()).await;
    app.get("/lazy-failure")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(row["context"]["userId"], "lazy-user");
}
#[tokio::test]
async fn explicit_request_and_loaned_facade_paths_can_both_resolve_lazy_identity() {
    let mut app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    lazy_auth(&app, calls.clone()).await;
    app.get("/lazy")
        .await
        .unwrap()
        .assert_content("lazy-context");
    app.get("/loaned")
        .await
        .unwrap()
        .assert_content("loaned-context");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn authentication_errors_do_not_replace_the_original_report_or_escape_context() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Auth::manager().unwrap().via_request("cached", |_| {
            Box::pin(async { Err(openrust::auth::AuthError::InvalidIdentity) })
        });
    })
    .await;
    app.get("/lazy-failure")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(row["message"], "500 Internal Server Error");
    assert!(row["context"].get("userId").is_none());
    assert!(contents.contains("context-failure"));
}
#[tokio::test]
async fn reporting_inside_lazy_auth_does_not_recursively_resolve_the_same_guard() {
    let mut app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    App::set_facade_application(Some(app.app()), async {
        Auth::manager().unwrap().via_request("cached", move |_| {
            let seen = seen.clone();
            Box::pin(async move {
                seen.fetch_add(1, Ordering::SeqCst);
                Exceptions::report(Failure)
                    .await
                    .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?;
                Ok(Some(Box::new(Account("recursive-user".into()))
                    as Box<dyn openrust::contracts::auth::Authenticatable>))
            })
        });
    })
    .await;
    app.get("/lazy-failure").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let rows = contents
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(rows[0]["context"].get("userId").is_none());
    assert_eq!(rows[1]["context"]["userId"], "recursive-user");
}
#[tokio::test]
async fn cancelled_auth_resolution_restores_the_task_local_guard_for_the_next_request() {
    let mut app = app().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let signal = entered.clone();
    App::set_facade_application(Some(app.app()), async {
        Auth::manager().unwrap().via_request("cached", move |_| {
            let signal = signal.clone();
            Box::pin(async move {
                signal.notify_one();
                std::future::pending().await
            })
        });
    })
    .await;
    {
        let request = app.get("/lazy-failure");
        tokio::pin!(request);
        tokio::select! {_ = entered.notified()=>{}, result=&mut request=>panic!("认证应当等待而非提前结束：{result:?}")}
    }
    let calls = Arc::new(AtomicUsize::new(0));
    lazy_auth(&app, calls.clone()).await;
    app.get("/lazy-failure").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(row["context"]["userId"], "lazy-user");
}

fn report_lazy_route(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Exceptions::report_for_request(request, Failure)
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        Ok(Response::from("reported-lazy"))
    })
}
#[tokio::test]
async fn public_contextual_report_can_resolve_identity_while_the_route_holds_the_request() {
    let mut app = app().await;
    let calls = Arc::new(AtomicUsize::new(0));
    lazy_auth(&app, calls.clone()).await;
    app.get("/report-lazy")
        .await
        .unwrap()
        .assert_content("reported-lazy");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(row["context"]["userId"], "lazy-user");
    assert_eq!(row["message"], "context-failure");
}
struct MissingMetadata;
impl ExceptionHandler for MissingMetadata {
    fn should_report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, bool> {
        Box::pin(async { true })
    }
    fn report<'a>(&'a self, _: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn render(&self, _: &Request, _: &Exception<'_>) -> Response {
        Response::from("minimal")
    }
    fn render_for_console(&self, _: &mut openrust::console::Output, _: &Exception<'_>) {}
}
#[tokio::test]
async fn custom_handlers_without_optional_metadata_fail_explicitly_instead_of_returning_fake_values()
 {
    let mut app = app().await;
    app.instance::<dyn ExceptionHandler>(Arc::new(MissingMetadata))
        .await
        .unwrap();
    for install_fake in [false, true] {
        if install_fake {
            app.fake_exceptions(&[]).await.unwrap();
        }
        App::set_facade_application(Some(app.app()), async {
            let error = Failure;
            let exception = Exception::Error(&error);
            assert!(Exceptions::register().is_err());
            assert!(Exceptions::build_context_for_exception(&exception).is_err());
            assert!(Exceptions::context_for_exception(&exception).await.is_err());
        })
        .await;
    }
}

#[tokio::test]
async fn nested_applications_resolve_their_own_auth_instead_of_sharing_a_recursion_flag() {
    let mut first = app().await;
    let second = app().await;
    let second_root = second.root().to_owned();
    App::set_facade_application(Some(second.app()), async {
        Auth::manager().unwrap().via_request("cached", |_| {
            Box::pin(async {
                Ok(Some(Box::new(Account("inner-user".into()))
                    as Box<dyn openrust::contracts::auth::Authenticatable>))
            })
        });
    })
    .await;
    let second = Arc::new(tokio::sync::Mutex::new(second));
    let nested = second.clone();
    App::set_facade_application(Some(first.app()), async {
        Auth::manager().unwrap().via_request("cached", move |_| {
            let nested = nested.clone();
            Box::pin(async move {
                nested
                    .lock()
                    .await
                    .get("/lazy-failure")
                    .await
                    .map_err(openrust::auth::AuthError::Backend)?
                    .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
                Ok(Some(Box::new(Account("outer-user".into()))
                    as Box<dyn openrust::contracts::auth::Authenticatable>))
            })
        });
    })
    .await;
    first.get("/lazy-failure").await.unwrap();
    let contents = std::fs::read_to_string(first.root().join("context.jsonl")).unwrap();
    let outer: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(outer["context"]["userId"], "outer-user");
    let contents = std::fs::read_to_string(second_root.join("context.jsonl")).unwrap();
    let inner: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(inner["context"]["userId"], "inner-user");
}

#[tokio::test]
async fn throw_on_report_inside_auth_is_caught_as_auth_failure_without_replacing_the_http_error() {
    let mut app = app().await;
    App::set_facade_application(Some(app.app()), async {
        Auth::manager().unwrap().via_request("cached", |_| {
            Box::pin(async {
                Exceptions::report(std::io::Error::other("auth-report"))
                    .await
                    .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?;
                panic!("认证内部的显式重抛必须中止该认证调用")
            })
        });
    })
    .await;
    let fake = app
        .fake_exceptions(&[openrust::testing::ExceptionType::of::<std::io::Error>()])
        .await
        .unwrap();
    fake.throw_on_report();
    for _ in 0..2 {
        app.get("/lazy-failure")
            .await
            .unwrap()
            .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
    fake.assert_reported::<std::io::Error>();
    fake.assert_reported_count(2);
    let contents = std::fs::read_to_string(app.root().join("context.jsonl")).unwrap();
    let rows = contents
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row["context"].get("userId").is_none())
    );
    assert!(contents.contains("context-failure"));
}
