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
            "app":{"env":"test","url":"http://localhost","key":"example-signed-test-key"},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "view":{"directory":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("modules/example/resources/views"),"auto_reload":false},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
        })))
}

#[tokio::test]
async fn example_signed_draft_routes_use_public_generator_request_and_middleware() -> TestResult {
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await?;
    let result = async {
        let issue = get(&app, "/api/example/draft/signed-links", None, None).await?;
        let links = state(&issue)?;
        let absolute = links["absolute"].as_str().ok_or("绝对签名链接缺失")?;
        let path = absolute
            .strip_prefix("http://localhost")
            .ok_or("签名链接 origin 错误")?;
        assert_eq!(state(&get(&app, path, None, None).await?)?["valid"], true);
        get(&app, &path.replace("/42", "/43"), None, None)
            .await?
            .assert_status(StatusCode::FORBIDDEN)?;
        let relative = links["relative"].as_str().ok_or("相对签名链接缺失")?;
        assert_eq!(
            state(&get(&app, relative, None, None).await?)?["valid"],
            true
        );
        get(
            &app,
            "/api/example/draft/signed/42?signature=invalid",
            None,
            None,
        )
        .await?
        .assert_status(StatusCode::FORBIDDEN)?;
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[cfg(feature = "view")]
async fn about<A: Application>(app: &TestApp<A>, cookie: Option<&str>) -> TestResult<TestResponse> {
    get(app, "/example/pages/about", cookie, None).await
}

async fn get<A: Application>(
    app: &TestApp<A>,
    path: &str,
    cookie: Option<&str>,
    referer: Option<&str>,
) -> TestResult<TestResponse> {
    let mut request = Request::get(path)
        .header("host", "localhost")
        .header(header::ACCEPT, "text/html");
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

async fn post_refresh<A: Application>(
    app: &TestApp<A>,
    cookie: Option<&str>,
) -> TestResult<TestResponse> {
    let mut request = Request::post("/api/example/draft/refresh?discard=once")
        .header("host", "localhost")
        .header(header::ACCEPT, "text/html")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app
        .request_with_limits(
            request.body(Body::from(r#"{"title":"  Recheck  "}"#))?,
            4096,
            TIMEOUT,
        )
        .await?)
}

async fn assert_refreshed_flash<A: Application>(app: &TestApp<A>) -> TestResult {
    let response = post_refresh(app, None).await?;
    redirect(&response, "/api/example/draft/refresh")?;
    let cookie = cookie_pair(&response)?;
    let page = get(app, "/api/example/draft/refresh", Some(&cookie), None).await?;
    assert_eq!(
        state(&page)?,
        json!({
            "draft":"示例草稿","old_title":"Recheck","title_error":"请检查草稿标题"
        })
    );
    let cookie = cookie_pair(&page)?;
    let aged = get(app, "/api/example/draft/refresh", Some(&cookie), None).await?;
    assert_eq!(
        state(&aged)?,
        json!({
            "draft":"示例草稿","old_title":null,"title_error":""
        })
    );
    Ok(())
}

async fn submit<A: Application>(
    app: &TestApp<A>,
    cookie: Option<&str>,
    value: Value,
) -> TestResult<TestResponse> {
    let mut request = Request::post("/api/example/draft/submit")
        .header("host", "localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json");
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
        assert_refreshed_flash(&app).await?;
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[tokio::test]
async fn draft_named_route_redirect_retains_flash_and_restores_custom_errors() -> TestResult {
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
        assert_eq!(aged["old"], json!([]));
        assert_eq!(aged["default"]["present"], false);
        assert_eq!(aged["other"]["present"], false);
        assert_action_flash(&app).await?;
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[cfg(feature = "view")]
#[tokio::test]
async fn draft_errors_render_per_request_for_two_clients_and_age() -> TestResult {
    let mut app = TestApp::<DraftApplication>::builder(sources())
        .build()
        .await?;
    let result = async {
        let initial_a = about(&app, None).await?;
        let initial_b = about(&app, None).await?;
        let mut a_cookie = cookie_pair(&initial_a)?;
        let mut b_cookie = cookie_pair(&initial_b)?;
        for initial in [&initial_a, &initial_b] {
            initial.assert_status(StatusCode::OK)?;
            let html = std::str::from_utf8(initial.body())?;
            assert!(html.contains("标题：</p>"));
            assert!(html.contains("摘要：</p>"));
            assert!(html.contains("不存在的字段：</p>"));
            assert!(!html.contains("id=\"draft-field-errors\""));
            assert!(!html.contains("<ul>"));
        }

        let a = submit(&app, Some(&a_cookie), json!({"mode":"errors","title":""})).await?;
        redirect(&a, "/api/example/draft/review")?;
        a_cookie = cookie_pair(&a)?;
        let b = submit(&app, Some(&b_cookie), json!({"mode":"custom"})).await?;
        redirect(&b, "/api/example/draft/review")?;
        b_cookie = cookie_pair(&b)?;

        let (a, b) = tokio::join!(about(&app, Some(&a_cookie)), about(&app, Some(&b_cookie)));
        let a = a?;
        let b = b?;
        a.assert_status(StatusCode::OK)?;
        b.assert_status(StatusCode::OK)?;
        let a_html = std::str::from_utf8(a.body())?;
        let b_html = std::str::from_utf8(b.body())?;
        assert!(a_html.contains("标题：标题不能为空"));
        assert!(a_html.contains("摘要：摘要需要补充"));
        assert!(a_html.contains("<li>摘要需要补充</li>"));
        assert!(!a_html.contains("草稿审核：请联系编辑"));
        assert!(b_html.contains("标题：草稿审核：请联系编辑"));
        assert!(b_html.contains("摘要：摘要需要补充"));
        assert!(b_html.contains("<li>摘要需要补充</li>"));
        assert!(!b_html.contains("标题不能为空"));
        for html in [a_html, b_html] {
            assert!(html.contains(
                "<p id=\"draft-field-errors\">标题或摘要存在审核问题，请修正后重新提交。</p>"
            ));
        }
        a_cookie = cookie_pair(&a)?;
        b_cookie = cookie_pair(&b)?;
        let (aged_a, aged_b) =
            tokio::join!(about(&app, Some(&a_cookie)), about(&app, Some(&b_cookie)));
        for aged in [aged_a?, aged_b?] {
            aged.assert_status(StatusCode::OK)?;
            let html = std::str::from_utf8(aged.body())?;
            assert!(html.contains("标题：</p>"));
            assert!(html.contains("摘要：</p>"));
            assert!(!html.contains("标题不能为空"));
            assert!(!html.contains("草稿审核：请联系编辑"));
            assert!(!html.contains("id=\"draft-field-errors\""));
            assert!(!html.contains("<ul>"));
        }
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}

#[tokio::test]
async fn plain_and_named_redirect_work_without_session_and_flash_reports_failure() -> TestResult {
    let mut app = TestApp::<crate::bootstrap::Server>::builder(sources())
        .build()
        .await?;
    let result = async {
        redirect(
            &get(&app, "/api/example/draft/plain", None, None).await?,
            "/api/example",
        )?;
        redirect(
            &get(&app, "/api/example/draft/named", None, None).await?,
            "/api/example",
        )?;
        let secure = get(&app, "/api/example/draft/secure", None, None).await?;
        secure.assert_status(StatusCode::FOUND)?;
        assert_eq!(
            secure
                .headers()
                .get(header::LOCATION)
                .ok_or("Location missing")?
                .to_str()?,
            "https://localhost/api/example"
        );
        let back = get(
            &app,
            "/api/example/draft/back",
            None,
            Some("/api/example/draft/plain"),
        )
        .await?;
        redirect(&back, "/api/example/draft/plain")?;
        assert_public_refresh_and_guide(&app).await?;
        assert_public_actions(&app).await?;
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

async fn assert_public_actions<A: Application>(app: &TestApp<A>) -> TestResult {
    let summary = get(app, "/api/example/draft/actions/summary", None, None).await?;
    assert_eq!(
        state(&summary)?,
        json!({"source":"模块草稿概览","action":"Example\\Draft\\DraftSummaryController","id":null})
    );
    for (entry, target, action) in [
        (
            "action",
            "/api/example/draft/actions/42?section=review",
            "Example\\Draft\\DraftController@show",
        ),
        (
            "action-preview",
            "/api/example/draft/actions/42/preview?section=review",
            "Example\\Draft\\PreviewDraftController",
        ),
    ] {
        let response = get(
            app,
            &format!("/api/example/draft/{entry}?url=https://untrusted.example"),
            None,
            None,
        )
        .await?;
        redirect(&response, target)?;
        let page = get(app, target, None, None).await?;
        assert_eq!(
            state(&page)?,
            json!({"draft":"示例动作草稿","action":action,"id":"42","old_title":null,"title_error":""})
        );
    }
    Ok(())
}

async fn assert_action_flash<A: Application>(app: &TestApp<A>) -> TestResult {
    let response = app
        .request_with_limits(
            Request::post("/api/example/draft/action-submit?url=https://untrusted.example")
                .header("host", "localhost")
                .header(header::ACCEPT, "text/html")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"title":"  Action draft  ","token":"private"}"#,
                ))?,
            4096,
            TIMEOUT,
        )
        .await?;
    response.assert_status(StatusCode::SEE_OTHER)?;
    assert_eq!(
        response.headers()[header::LOCATION],
        "http://localhost/api/example/draft/actions/42?section=review"
    );
    assert_eq!(response.headers()["x-example-draft-action"], "review");
    let cookie = cookie_pair(&response)?;
    let page = get(
        app,
        "/api/example/draft/actions/42?section=review",
        Some(&cookie),
        None,
    )
    .await?;
    let current = state(&page)?;
    assert_eq!(current["action"], "Example\\Draft\\DraftController@show");
    assert_eq!(current["id"], "42");
    assert_eq!(current["old_title"], "Action draft");
    assert_eq!(current["title_error"], "请复核动作草稿标题");
    let cookie = cookie_pair(&page)?;
    let aged = state(&get(app, "/api/example/draft/actions/42", Some(&cookie), None).await?)?;
    assert_eq!(aged["old_title"], Value::Null);
    assert_eq!(aged["title_error"], "");
    Ok(())
}

async fn assert_public_refresh_and_guide<A: Application>(app: &TestApp<A>) -> TestResult {
    let refreshed = post_refresh(app, None).await?;
    redirect(&refreshed, "/api/example/draft/refresh")?;
    let page = get(app, "/api/example/draft/refresh", None, None).await?;
    assert_eq!(
        state(&page)?,
        json!({
            "draft":"示例草稿","old_title":null,"title_error":""
        })
    );
    let guide = get(
        app,
        "/api/example/draft/guide?url=https://untrusted.example.test",
        None,
        None,
    )
    .await?;
    guide.assert_status(StatusCode::SEE_OTHER)?;
    assert_eq!(
        guide
            .headers()
            .get(header::LOCATION)
            .ok_or("Location missing")?
            .to_str()?,
        "https://laravel.com/docs/11.x/redirects?source=openrust&section=draft#redirecting-to-external-domains"
    );
    assert_eq!(
        guide
            .headers()
            .get("x-example-guide")
            .ok_or("guide header missing")?
            .to_str()?,
        "official"
    );
    Ok(())
}

async fn confirm<A: Application>(app: &TestApp<A>, cookie: &str) -> TestResult<TestResponse> {
    Ok(app
        .request_with_limits(
            Request::post("/api/example/draft/confirm")
                .header("host", "localhost")
                .header(header::ACCEPT, "text/html")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .body(Body::from("{}"))?,
            4096,
            TIMEOUT,
        )
        .await?)
}

#[tokio::test]
async fn draft_guest_intended_survives_refresh_and_is_consumed_per_cookie() -> TestResult {
    let mut app = TestApp::<DraftApplication>::builder(sources())
        .build()
        .await?;
    let result = async {
        let paths = [
            "/api/example/draft/preview?section=summary",
            "/api/example/draft/preview?section=history",
        ];
        let a = get(&app, paths[0], None, None).await?;
        let b = get(&app, paths[1], None, None).await?;
        redirect(&a, "/api/example/draft/confirm")?;
        redirect(&b, "/api/example/draft/confirm")?;
        let mut cookies = [cookie_pair(&a)?, cookie_pair(&b)?];
        assert_ne!(cookies[0], cookies[1]);
        // 两次普通 GET 后仍保留意图；若误用 flash，第二次读取会丢失。
        for _ in 0..2 {
            for (cookie, path) in cookies.iter_mut().zip(paths) {
                let page = get(&app, "/api/example/draft/confirm", Some(cookie), None).await?;
                assert_eq!(state(&page)?["intended"], format!("http://localhost{path}"));
                *cookie = cookie_pair(&page)?;
            }
        }
        for (index, path) in paths.into_iter().enumerate() {
            let response = confirm(&app, &cookies[index]).await?;
            redirect(&response, path)?;
            cookies[index] = cookie_pair(&response)?;
            let preview = get(&app, path, Some(&cookies[index]), None).await?;
            assert_eq!(state(&preview)?["draft"], "示例草稿");
            assert_eq!(state(&preview)?["preview_confirmed"], true);
            cookies[index] = cookie_pair(&preview)?;
            let page = get(
                &app,
                "/api/example/draft/confirm",
                Some(&cookies[index]),
                None,
            )
            .await?;
            assert_eq!(state(&page)?["intended"], Value::Null);
            cookies[index] = cookie_pair(&page)?;
            let repeated = confirm(&app, &cookies[index]).await?;
            redirect(&repeated, "/api/example/draft/review")?;
            cookies[index] = cookie_pair(&repeated)?;
            if index == 0 {
                let waiting =
                    get(&app, "/api/example/draft/confirm", Some(&cookies[1]), None).await?;
                assert_eq!(
                    state(&waiting)?["intended"],
                    format!("http://localhost{}", paths[1])
                );
                cookies[1] = cookie_pair(&waiting)?;
            }
        }
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + TIMEOUT).await?;
    result
}
