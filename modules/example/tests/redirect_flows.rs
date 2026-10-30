//! 生产模块路由的草稿重定向，验证跨请求 flash、命名包及自定义包恢复。

use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::core::{Application, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use openrust::session::{MemorySessionStore, SessionServiceProvider};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const COOKIE: &str = "example_draft_session";
const TIMEOUT: Duration = Duration::from_secs(5);

struct DraftApplication;
impl Application for DraftApplication {
    type Config = <crate::bootstrap::Server as Application>::Config;
    fn config_namespace() -> &'static str {
        crate::bootstrap::Server::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = crate::bootstrap::Server::providers(config);
        providers.push(Box::new(
            SessionServiceProvider::with_store(MemorySessionStore::default()).with_cookie_config(
                openrust::config::SessionConfig {
                    driver: Some("array".into()),
                    cookie: Some(COOKIE.into()),
                    secure: Some(false),
                    ..openrust::config::SessionConfig::default()
                },
            ),
        ));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        crate::bootstrap::Server::modules(config)
    }
}

fn sources() -> Figment {
    Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(json!({
            "app":{"env":"test"},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
}

async fn get<A: Application>(
    app: &TestApp<A>,
    path: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
) -> TestResult<TestResponse> {
    let mut request = Request::get(path).header("host", "localhost");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    if let Some(referer) = referer {
        request = request.header(header::REFERER, referer);
    }
    Ok(app
        .request_with_limits(request.body(Body::empty())?, 4096, TIMEOUT)
        .await?)
}

async fn submit<A: Application>(
    app: &TestApp<A>,
    cookie: Option<&str>,
    value: Value,
) -> TestResult<TestResponse> {
    let mut request = Request::post("/api/example/draft/submit")
        .header("host", "localhost")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app
        .request_with_limits(
            request.body(Body::from(serde_json::to_vec(&value)?))?,
            4096,
            TIMEOUT,
        )
        .await?)
}

fn cookie_pair(response: &TestResponse) -> TestResult<String> {
    let header = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with(COOKIE))
        .ok_or("draft Session Cookie missing")?;
    Ok(header
        .split(';')
        .next()
        .ok_or("draft Cookie malformed")?
        .to_owned())
}

fn redirect(response: &TestResponse, path: &str) -> TestResult {
    response.assert_status(StatusCode::FOUND)?;
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .ok_or("Location missing")?
            .to_str()?,
        format!("http://localhost{path}")
    );
    Ok(())
}

fn state(response: &TestResponse) -> TestResult<Value> {
    response.assert_status(StatusCode::OK)?;
    let body: Value = response.json()?;
    Ok(body["data"].clone())
}

#[tokio::test]
async fn draft_redirect_flash_modes_and_two_clients_age_independently() -> TestResult {
    let mut app = TestApp::<DraftApplication>::builder(sources())
        .build()
        .await?;
    let result = async {
        let a = get(&app, "/api/example/draft/review", None, None).await?;
        let mut a_cookie = cookie_pair(&a)?;
        let b = get(&app, "/api/example/draft/review", None, None).await?;
        let mut b_cookie = cookie_pair(&b)?;

        let a = submit(
            &app,
            Some(&a_cookie),
            json!({"mode":"only","title":"  Alpha  ","summary":"Ready","token":"private"}),
        )
        .await?;
        redirect(&a, "/api/example/draft/review")?;
        a_cookie = cookie_pair(&a)?;
        let b = submit(
            &app,
            Some(&b_cookie),
            json!({"mode":"except","title":"Beta","token":"secret"}),
        )
        .await?;
        redirect(&b, "/api/example/draft/review")?;
        b_cookie = cookie_pair(&b)?;

        let a = get(&app, "/api/example/draft/review", Some(&a_cookie), None).await?;
        assert_eq!(
            state(&a)?["old"],
            json!({"title":"Alpha","summary":"Ready"})
        );
        a_cookie = cookie_pair(&a)?;
        let b = get(&app, "/api/example/draft/review", Some(&b_cookie), None).await?;
        assert_eq!(state(&b)?["old"], json!({"title":"Beta"}));
        b_cookie = cookie_pair(&b)?;
        assert_eq!(
            state(&get(&app, "/api/example/draft/review", Some(&a_cookie), None).await?)?["old"],
            json!([])
        );
        assert_eq!(
            state(&get(&app, "/api/example/draft/review", Some(&b_cookie), None).await?)?["old"],
            json!([])
        );

        let a = submit(
            &app,
            Some(&a_cookie),
            json!({"mode":"all","title":"Gamma","token":"visible"}),
        )
        .await?;
        a_cookie = cookie_pair(&a)?;
        let a = get(&app, "/api/example/draft/review", Some(&a_cookie), None).await?;
        assert_eq!(
            state(&a)?["old"],
            json!({"mode":"all","title":"Gamma","token":"visible"})
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[tokio::test]
async fn draft_redirect_retains_named_errors_and_restores_custom_behavior() -> TestResult {
    let mut app = TestApp::<DraftApplication>::builder(sources())
        .build()
        .await?;
    let result = async {
        let initial = get(&app, "/api/example/draft/review", None, None).await?;
        let mut cookie = cookie_pair(&initial)?;
        let response = submit(&app, Some(&cookie), json!({"mode":"errors","title":""})).await?;
        redirect(&response, "/api/example/draft/review")?;
        cookie = cookie_pair(&response)?;
        let error_state = get(&app, "/api/example/draft/review", Some(&cookie), None).await?;
        let ordinary = state(&error_state)?;
        assert_eq!(ordinary["old"], json!({"title":null}));
        assert_eq!(ordinary["default"]["title"], "标题不能为空");
        assert_eq!(ordinary["other"]["message"], "摘要需要补充");
        cookie = cookie_pair(&error_state)?;
        let response = submit(&app, Some(&cookie), json!({"mode":"custom"})).await?;
        cookie = cookie_pair(&response)?;
        let response = get(&app, "/api/example/draft/review", Some(&cookie), None).await?;
        let current = state(&response)?;
        assert_eq!(current["default"]["title"], "草稿审核：请联系编辑");
        assert_eq!(
            current["other"],
            json!({"present":true,"message":"摘要需要补充"})
        );
        cookie = cookie_pair(&response)?;
        let aged = state(&get(&app, "/api/example/draft/review", Some(&cookie), None).await?)?;
        assert_eq!(aged["default"]["present"], false);
        assert_eq!(aged["other"]["present"], false);
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[tokio::test]
async fn plain_redirect_works_without_session_and_flash_reports_failure() -> TestResult {
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await?;
    let result = async {
        redirect(
            &get(&app, "/api/example/draft/plain", None, None).await?,
            "/api/example",
        )?;
        let back = get(
            &app,
            "/api/example/draft/back",
            None,
            Some("/api/example/draft/plain"),
        )
        .await?;
        redirect(&back, "/api/example/draft/plain")?;
        let failed = submit(&app, None, json!({"mode":"only","title":"Alpha"})).await?;
        failed.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
        let body: Value = failed.json()?;
        assert_eq!(body["code"], "session_layer_missing");
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}
