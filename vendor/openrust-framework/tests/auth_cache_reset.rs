use openrust::{
    auth::{AuthIdentifier, AuthManager},
    contracts::{
        auth::{Authenticatable, Factory as _},
        session::Session as _,
    },
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Auth, Route},
    testing::TestApp,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Identity(String);
impl Authenticatable for Identity {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
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
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/reset", reset)?;
    Route::get("/repeat", repeat)?;
    Ok(())
}
fn reset(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let before = Auth::guard(request, Some("first"))?
            .user()
            .await?
            .unwrap()
            .get_auth_identifier();
        Auth::should_use(request, "second")?;
        assert_eq!(
            Auth::user(request).await?.unwrap().get_auth_identifier(),
            before
        );
        let changed = format!("{before}-fresh");
        request
            .parts_mut()
            .headers
            .insert("x-subject", changed.parse().unwrap());
        assert_eq!(
            Auth::user(request).await?.unwrap().get_auth_identifier(),
            before
        );
        assert!(Auth::resolved_user(None)?.is_some());
        let session_id = request.session()?.get_id().to_owned();
        let csrf = request.session()?.get("_token").cloned();
        let cookie = request.header("cookie")?.map(str::to_owned);
        Auth::forget_guards(request)?;
        assert_eq!(request.session()?.get_id(), session_id);
        assert_eq!(request.session()?.get("_token"), csrf.as_ref());
        assert_eq!(request.header("cookie")?.map(str::to_owned), cookie);
        assert!(Auth::resolved_user(None)?.is_none());
        assert_eq!(Auth::guard_name(request, None)?, "second");
        assert_eq!(
            Auth::user(request).await?.unwrap().get_auth_identifier(),
            AuthIdentifier::from(changed.clone())
        );
        assert_eq!(
            Auth::guard(request, Some("first"))?
                .user()
                .await?
                .unwrap()
                .get_auth_identifier(),
            AuthIdentifier::from(changed.clone())
        );
        Response::json(
            StatusCode::OK,
            json!({"before":before,"after":changed,"guard":Auth::guard_name(request,None)?}),
        )
    })
}
fn repeat(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        for _ in 0..2 {
            assert!(Auth::user(request).await?.is_none());
        }
        Auth::forget_guards(request)?;
        for _ in 0..2 {
            assert!(Auth::user(request).await?.is_none());
        }
        Response::json(StatusCode::OK, json!({"guest":true}))
    })
}
async fn application(counter: Arc<AtomicUsize>) -> TestApp {
    let mut app = TestApp::create(|builder|builder.with_config(|_,_|Ok([
      ("app".into(),json!({"key":"a".repeat(32),"url":"http://localhost","debug":true})),
      ("session".into(),json!({"driver":"array","cookie":"auth_cache_test","lottery":[0,100]})),
      ("auth".into(),json!({"defaults":{"guard":"first"},"guards":{"first":{"driver":"header"},"second":{"driver":"header"}}})),
    ].into())).with_routing(Routing::default().web(routes))).unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<AuthManager>().await.unwrap();
    manager.via_request("header", move |request| {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(request
                .header("x-subject")?
                .map(|value| Box::new(Identity(value.into())) as Box<dyn Authenticatable>))
        })
    });
    app
}
#[tokio::test]
async fn facade_resets_all_request_guards_preserving_selection_and_other_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut app = application(calls.clone()).await;
    for subject in ["one", "two"] {
        let response = app
            .call(
                http::Request::get("/reset")
                    .header("x-subject", subject)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response.assert_ok().assert_json(
            &json!({"before":subject,"after":format!("{subject}-fresh"),"guard":"second"}),
            true,
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 8);
    assert_eq!(
        app.app()
            .make::<AuthManager>()
            .await
            .unwrap()
            .get_default_driver(),
        "first"
    );
    let other_calls = Arc::new(AtomicUsize::new(0));
    let mut other = application(other_calls.clone()).await;
    other.get("/repeat").await.unwrap().assert_ok();
    assert_eq!(other_calls.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 8);
}
#[tokio::test]
async fn manager_cache_reset_is_idempotent_for_empty_request_and_keeps_guest_cache_local() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut app = application(calls.clone()).await;
    let manager = app.app().make::<AuthManager>().await.unwrap();
    let (parts, body) = http::Request::new(Body::empty()).into_parts();
    let mut request = Request::from_parts(parts, body);
    manager.forget_guards(&mut request);
    manager.forget_guards(&mut request);
    app.get("/repeat").await.unwrap().assert_ok();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn forgetting_inside_a_resolver_keeps_reentrant_authentication_bounded() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = application(calls.clone()).await;
    let manager = app.app().make::<AuthManager>().await.unwrap();
    let weak = Arc::downgrade(&manager);
    let seen = calls.clone();
    manager.via_request("header", move |request| {
        let weak = weak.clone();
        let seen = seen.clone();
        Box::pin(async move {
            assert!(
                seen.fetch_add(1, Ordering::SeqCst) < 2,
                "缓存重置丢失了认证重入保护"
            );
            let manager = weak.upgrade().unwrap();
            manager.forget_guards(request);
            assert!(manager.guard(request, None)?.user().await?.is_none());
            Ok(None)
        })
    });
    let (parts, body) = http::Request::new(Body::empty()).into_parts();
    let mut request = Request::from_parts(parts, body);
    for _ in 0..2 {
        assert!(
            manager
                .guard(&mut request, None)
                .unwrap()
                .user()
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    manager.forget_guards(&mut request);
    assert!(
        manager
            .guard(&mut request, None)
            .unwrap()
            .user()
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
