//! 通过生产应用装配验证模块 Cookie 公开接口；TestApp 使用进程内 HTTP 路由。

use super::config_sources::enabled_example_sources;
use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn sources() -> Figment {
    Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(json!({
        "app": {"env": "test"},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {
            "active_key_id": "test",
            "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}
        }}
    })))
}

fn success(response: &TestResponse, data: &Value) -> openrust::Result<()> {
    response
        .assert_status(StatusCode::OK)?
        .assert_json(&json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": data,
        }))?;
    Ok(())
}

async fn get(
    app: &TestApp<crate::bootstrap::Server>,
    cookie: Option<&str>,
) -> TestResult<TestResponse> {
    let mut request = Request::get("/api/example/cookie").header("host", "localhost");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app.request(request.body(Body::empty())?).await?)
}

async fn post(
    app: &TestApp<crate::bootstrap::Server>,
    action: &str,
    value: &str,
) -> TestResult<TestResponse> {
    let body = serde_json::to_vec(&json!({"action": action, "value": value}))?;
    Ok(app
        .request(
            Request::post("/api/example/cookie")
                .header("host", "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))?,
        )
        .await?)
}

fn wire_cookie(response: &TestResponse) -> TestResult<String> {
    let header = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|header| {
            header
                .to_str()
                .ok()
                .filter(|value| value.starts_with("example_preference="))
        })
        .ok_or("example preference Set-Cookie missing")?;
    let lower = header.to_ascii_lowercase();
    assert!(lower.contains("path=/api/example"));
    assert!(lower.contains("httponly"));
    assert!(lower.contains("samesite=lax"));
    Ok(header
        .split(';')
        .next()
        .ok_or("cookie pair missing")?
        .into())
}

#[tokio::test]
async fn module_cookie_queue_direct_forget_and_request_isolation() -> TestResult {
    let mut app =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources()))?)
            .build()
            .await?;
    let result = async {
        success(
            &get(&app, None).await?,
            &json!({"preference": null, "present": false}),
        )?;

        let value = "blue+green;warm";
        for (action, queued) in [("queue", true), ("direct", false)] {
            let written = post(&app, action, value).await?;
            success(&written, &json!({"action": action, "queued": queued}))?;
            let cookie = wire_cookie(&written)?;
            assert!(!cookie.contains(value));
            assert!(cookie.starts_with("example_preference="));
            success(
                &get(&app, Some(&cookie)).await?,
                &json!({"preference": value, "present": true}),
            )?;
            // 新请求没有携带 Cookie 时，先前队列不可串到当前请求。
            success(
                &get(&app, None).await?,
                &json!({"preference": null, "present": false}),
            )?;
        }

        let forgotten = post(&app, "forget", "").await?;
        success(&forgotten, &json!({"action": "forget", "queued": true}))?;
        let expired = forgotten
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|header| {
                header
                    .to_str()
                    .ok()
                    .filter(|value| value.starts_with("example_preference="))
            })
            .ok_or("expiration Set-Cookie missing")?;
        let expired = expired.to_ascii_lowercase();
        assert!(expired.contains("path=/api/example"));
        assert!(expired.contains("max-age=0"));
        success(
            &get(&app, None).await?,
            &json!({"preference": null, "present": false}),
        )?;

        let invalid = post(&app, "unknown", "sentinel").await?;
        invalid.assert_status(StatusCode::BAD_REQUEST)?;
        let body: Value = invalid.json()?;
        assert_eq!(body["code"], "example_cookie_action_invalid");
        assert!(!String::from_utf8_lossy(invalid.body()).contains("sentinel"));
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}
