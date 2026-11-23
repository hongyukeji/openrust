#![allow(non_snake_case, reason = "保留既定公开接口及命名")]
use openrust::{
    contracts::session::Session as _,
    foundation::configuration::Routing,
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RouteGroup,
    support::facades::{Broadcast, Route},
    testing::TestApp,
};
use serde_json::{Value, json};
use std::sync::Arc;
fn seed(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        request.session_mut()?.flash("notice", json!("keep"))?;
        Ok(Response::from("seed"))
    })
}
fn peek(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({"notice":request.session()?.get("notice")}),
        )
    })
}
#[tokio::test]
async fn user_routes_use_named_connections_request_identity_signatures_and_session_reflash() {
    let mut app = TestApp::create(|builder| builder.with_config(|_,_|Ok([
        ("app".into(),json!({"env":"testing","key":"12345678901234567890123456789012"})),
        ("session".into(),json!({"driver":"array","cookie":"user-auth-session"})),
        ("broadcasting".into(),json!({"default":"one","connections":{
            "one":{"driver":"pusher","key":"one-key","secret":"one-secret","app_id":"one","options":{"host":"127.0.0.1","scheme":"http","port":6001}},
            "two":{"driver":"reverb","key":"two-key","secret":"two-secret","app_id":"two","options":{"host":"127.0.0.1","scheme":"http","port":6001}}
        }}))
    ].into())).with_providers(vec![Box::new(openrust_reverb::ReverbServiceProvider)])
    .with_routing(Routing::default().web(|| {
        Route::get("/seed",seed)?;Route::get("/peek",peek)?;
        Broadcast::userRoutes(None,None)?;
        Broadcast::userRoutes(Some(RouteGroup::new().prefix("custom").middleware_name("web")),Some("two"))
    })) ).unwrap();
    app.boot().await.unwrap();
    app.app()
        .call(
            openrust::container::Callback::new(&[], |_| async {
                Broadcast::resolve_authenticated_user_using(
                    Arc::new(|request| {
                        Box::pin(async move {
                            Ok(request
                                .headers()
                                .get("x-user")
                                .and_then(|v| v.to_str().ok())
                                .map(|id| json!({"id":id})))
                        })
                    }),
                    None,
                )
                .unwrap();
                Broadcast::resolve_authenticated_user_using(
                    Arc::new(|_| Box::pin(async { Ok(Some(json!({"id":"named"}))) })),
                    Some("two"),
                )
                .unwrap();
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let seeded = app.get("http://example.test/seed").await.unwrap();
    let cookie = seeded
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("user-auth-session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let auth = app
        .call(
            axum::http::Request::post("http://example.test/broadcasting/user-auth")
                .header("content-type", "application/json")
                .header("x-user", "42")
                .header("cookie", &cookie)
                .body(Body::from(r#"{"socket_id":"123.456"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    auth.assert_ok()
        .assert_session_has("notice", Some(&json!("keep")));
    let data: Value = auth.json().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(data["user_data"].as_str().unwrap()).unwrap(),
        json!({"id":"42"})
    );
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, b"one-secret");
    let payload = format!("123.456::user::{}", data["user_data"].as_str().unwrap());
    let mac = aws_lc_rs::hmac::sign(&key, payload.as_bytes());
    let hex: String = mac.as_ref().iter().map(|v| format!("{v:02x}")).collect();
    assert_eq!(data["auth"], format!("one-key:{hex}"));
    app.call(
        axum::http::Request::get("http://example.test/peek")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_json_path("notice", &json!("keep"));
    app.get("http://example.test/broadcasting/user-auth?socket_id=123.456")
        .await
        .unwrap()
        .assert_status(StatusCode::FORBIDDEN);
    app.call(
        axum::http::Request::get("http://example.test/broadcasting/user-auth?socket_id=invalid")
            .header("x-user", "42")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_status(StatusCode::FORBIDDEN);
    let named = app
        .get("http://example.test/custom/broadcasting/user-auth?socket_id=123.456")
        .await
        .unwrap();
    named.assert_ok();
    let named: Value = named.json().unwrap();
    assert!(named["auth"].as_str().unwrap().starts_with("two-key:"));
    assert_eq!(
        serde_json::from_str::<Value>(named["user_data"].as_str().unwrap()).unwrap(),
        json!({"id":"named"})
    );
}
