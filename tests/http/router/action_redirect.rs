//! 动作声明、真实匹配和请求绑定重定向的 HTTP 接线。

use super::*;
use openrust::http::{Redirector, RequestInfo};
use std::collections::BTreeMap;

struct ActionNamespace;
impl openrust::core::ServiceProvider for ActionNamespace {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.set_root_controller_namespace("Http\\Draft")?;
            Ok(())
        })
    }
}

async fn target(info: RequestInfo) -> openrust::Result<Json<serde_json::Value>> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("test_action_route_missing", "实际动作路由缺失"))?;
    Ok(Json(serde_json::json!({
        "action": route.action(), "name":route.name(), "id":route.parameter("id"),
    })))
}

async fn business_error(info: RequestInfo, Path(code): Path<String>) -> openrust::Result<Response> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("test_action_route_missing", "实际动作路由缺失"))?;
    let action = route
        .action()
        .ok_or_else(|| openrust::Error::custom("test_action_missing", "实际动作缺失"))?;
    let mut response = (
        if code == "404" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::METHOD_NOT_ALLOWED
        },
        "业务终止",
    )
        .into_response();
    response.headers_mut().insert(
        "x-action-proof",
        action.parse().map_err(openrust::Error::internal)?,
    );
    Ok(response)
}

async fn jump(info: RequestInfo, Path(mode): Path<String>) -> openrust::Result<Response> {
    #[cfg(feature = "session")]
    assert!(!info.has_session(), "普通动作跳转不依赖 Session Provider");
    let params = BTreeMap::from([
        ("id".into(), "a b".into()),
        ("filter".into(), "tea & cake".into()),
    ]);
    let redirector = Redirector::from_info(&info);
    let response = match mode.as_str() {
        "tuple" => redirector.action(("Http\\Draft\\DraftController", "show"), &params)?,
        "invoke" => redirector.action("PreviewDraftController", &params)?,
        "unknown" => redirector.action_with(
            "Unknown@show",
            &params,
            StatusCode::OK,
            axum::http::HeaderMap::new(),
        )?,
        "missing" => redirector.action("DraftController@show", &BTreeMap::new())?,
        "custom" => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                "x-action-proof",
                "preserved".parse().map_err(openrust::Error::internal)?,
            );
            headers.insert(
                header::LOCATION,
                "https://ignored.example"
                    .parse()
                    .map_err(openrust::Error::internal)?,
            );
            redirector.action_with(
                "\\Http\\Draft\\DraftController@show",
                &params,
                StatusCode::SEE_OTHER,
                headers,
            )?
        }
        _ => redirector.action("DraftController@show", &params)?,
    };
    Ok(response.into_response())
}

async fn action_router() -> Result<axum::Router, Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.app.url = "https://configured.example:8443/base".into();
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(ActionNamespace)
        .provider(TestRoutes::new(|routes| {
            let routes = routes
                .get("/jump/{mode}", jump)
                .get_action("/business/{status}", "Business@show", business_error)
                .group(
                    RouteGroup::new().prefix("/drafts").namespace("Http\\Draft"),
                    |drafts| {
                        drafts
                            .get_action("/{id}", "DraftController@show", target)
                            .get_action("/{id}/preview", "PreviewDraftController", target)
                    },
                );
            #[cfg(feature = "session")]
            let routes = routes.post("/submit", submit);
            routes
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    Ok(build_router(&app)?)
}

#[tokio::test]
async fn anonymous_action_redirect_uses_request_generator_without_session() -> TestResult {
    let router = action_router().await?;
    for (mode, suffix, status) in [
        ("string", "a%20b", StatusCode::FOUND),
        ("tuple", "a%20b", StatusCode::FOUND),
        ("invoke", "a%20b/preview", StatusCode::FOUND),
        ("custom", "a%20b", StatusCode::SEE_OTHER),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/jump/{mode}"))
                    .header(header::HOST, "attacker.example")
                    .header(header::ACCEPT, "text/html")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers()[header::LOCATION],
            format!("https://configured.example:8443/base/drafts/{suffix}?filter=tea+%26+cake")
        );
        if mode == "custom" {
            assert_eq!(response.headers()["x-action-proof"], "preserved");
        }
    }
    for (path, action) in [
        ("/drafts/a%20b", "Http\\Draft\\DraftController@show"),
        (
            "/drafts/a%20b/preview",
            "Http\\Draft\\PreviewDraftController",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, "configured.example")
                    .header(header::ACCEPT, "application/json")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await["data"],
            serde_json::json!({"action":action,"name":null,"id":"a b"})
        );
    }
    for status in [StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/business/{}", status.as_u16()))
                    .header(header::HOST, "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["x-action-proof"], "Business@show");
    }
    Ok(())
}

async fn missing_generator(info: RequestInfo) -> openrust::Result<Response> {
    // 独立 RequestInfo 不需要路径提取；直接核前置 Result，而非从 HTTP 状态猜错误。
    let error = Redirector::from_info(&info)
        .action("DraftController@show", &BTreeMap::new())
        .err()
        .ok_or_else(|| {
            openrust::Error::custom("test_generator_unexpected", "独立请求不应有生成器")
        })?;
    assert_eq!(error.code(), "http_route_url_unavailable");
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    Err(error)
}

#[tokio::test]
async fn action_redirect_generation_errors_precede_response_validation() -> TestResult {
    let router = action_router().await?;
    for (mode, code) in [
        ("unknown", "http_route_action_not_found"),
        ("missing", "http_route_parameter_missing"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/jump/{mode}"))
                    .header(header::HOST, "localhost")
                    .header(header::ACCEPT, "application/json")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!response.headers().contains_key(header::LOCATION));
        assert_eq!(json_body(response).await["code"], code);
    }
    // 在应用 Router 之外，可信 Route 快照并不能凭空提供 URL 索引。
    let bare =
        axum::Router::new().route("/without-generator", axum::routing::get(missing_generator));
    let response = bare
        .oneshot(
            Request::get("/without-generator")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!response.headers().contains_key(header::LOCATION));
    assert_eq!(
        json_body(response).await["code"],
        "http_route_url_unavailable"
    );
    Ok(())
}

async fn lookup(urls: RouteUrlGenerator, Path(which): Path<String>) -> openrust::Result<String> {
    let action = match which.as_str() {
        "old" => "Old@show",
        "new" => "New@show",
        "shared" => "Shared@show",
        _ => "Domain@show",
    };
    urls.action(action, &BTreeMap::new())
}

#[tokio::test]
async fn action_index_uses_effective_handler_and_domain_first_registration() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.app.url = "http://localhost".into();
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/lookup/{which}", lookup)
                .get_action("/replaced", "Old@show", target)
                .get_action("/replaced", "New@show", target)
                .get_action("/first", "Shared@show", target)
                .get_action("/second", "Shared@show", target)
                .get_action("/root", "Domain@show", target)
                .group(RouteGroup::new().domain("shop.example"), |routes| {
                    routes.get_action("/domain", "Domain@show", target)
                })
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for (which, expected) in [
        ("new", "http://localhost/replaced"),
        ("shared", "http://localhost/first"),
        ("domain", "http://shop.example/domain"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/lookup/{which}"))
                    .header(header::HOST, "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
    }
    let old = router
        .clone()
        .oneshot(
            Request::get("/lookup/old")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(old.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(old).await["code"], "http_route_action_not_found");
    let actual = router
        .oneshot(
            Request::get("/replaced")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(json_body(actual).await["data"]["action"], "New@show");
    Ok(())
}

#[cfg(feature = "session")]
async fn submit(request: openrust::http::Request) -> openrust::Result<Response> {
    let mut response = Redirector::from_request(&request).action(
        ("Http\\Draft\\DraftController", "show"),
        &BTreeMap::from([("id".into(), "42".into())]),
    )?;
    response
        .with_input()?
        .with_errors(openrust::http::RedirectErrors::Text("检查标题"), "default")?;
    Ok(response.into_response())
}

#[cfg(feature = "session")]
#[tokio::test]
async fn action_redirect_from_request_keeps_same_session_input_and_errors() -> TestResult {
    // 此层验证提取器/借用接线，真实 Cookie 与老化流程由唯一模块和生成应用覆盖。
    let session = openrust::session::Session::new(
        None,
        Arc::new(openrust::session::MemorySessionStore::default()),
        None,
    )?;
    let router = action_router().await?.layer(Extension(session.clone()));
    let response = router
        .oneshot(
            Request::post("/submit")
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/json")
                .body(Body::from(r#"{"title":"  Draft  "}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers()[header::LOCATION],
        "https://configured.example:8443/base/drafts/42"
    );
    assert_eq!(
        session.get_old_input_json(Some("title"))?,
        Some(serde_json::json!("Draft"))
    );
    assert_eq!(
        session.read_errors()?.default_first(None, None)?,
        Some("检查标题".into())
    );
    Ok(())
}
