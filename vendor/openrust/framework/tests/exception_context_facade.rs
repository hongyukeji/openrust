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
 ].into())).with_routing(Routing::default().web(||Route::get("/context/{id}",context_route)))).unwrap();
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
        let full = Exceptions::context_for_exception(&exception).unwrap();
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
    fn build_context_for_exception(&self, _: &Exception<'_>) -> LogContext {
        LogContext::from_iter([("custom".into(), json!(true))])
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
        let full = Exceptions::context_for_exception(&exception).unwrap();
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
