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
        "app": {"env": "test", "key": "0123456789abcdef0123456789abcdef"},
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

#[cfg(feature = "encryption")]
#[tokio::test]
async fn contextual_cookie_first_use_rejects_invalid_key_after_ordinary_route_succeeds()
-> TestResult {
    let config = sources().merge(Serialized::defaults(json!({
        "security": {"encryption": {
            "active_key_id": "test",
            "keys": {"test": "example-invalid-key-sentinel"}
        }}
    })));
    let mut builder =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&config)?);
    builder.configure_providers(|providers| {
        providers.force_provider(openrust::http::CookieServiceProvider::contextual());
    });
    let mut app = builder.build().await?;
    let result: TestResult = async {
        let ordinary = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        ordinary.assert_status(StatusCode::OK)?;
        assert!(!ordinary.headers().contains_key(header::SET_COOKIE));
        let protected = get(&app, None).await?;
        protected.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
        assert!(!protected.headers().contains_key(header::SET_COOKIE));
        assert_eq!(protected.headers()[header::CACHE_CONTROL], "no-store");
        let payload: Value = protected.json()?;
        assert_eq!(payload["code"], "service_initialization_failed");
        assert!(
            !String::from_utf8_lossy(protected.body()).contains("example-invalid-key-sentinel")
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn example_application_resolves_laravel_string_encryption_from_app_config() -> TestResult {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use openrust::encryption::LaravelEncrypter;
    let config = sources().merge(Serialized::defaults(json!({
        "app": {"cipher": "AES-128-GCM", "key": "0123456789abcdef"},
    })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&config)?)
        .build()
        .await?;
    let result: TestResult = async {
        let service = app.app()?.state().resolve::<LaravelEncrypter>().await?;
        let wire = service.encrypt_string(b"example application payload")?;
        let fields: Value = serde_json::from_slice(&STANDARD.decode(&wire)?)?;
        let iv = fields["iv"].as_str().ok_or("Laravel payload missing IV")?;
        let tag = fields["tag"]
            .as_str()
            .ok_or("Laravel payload missing tag")?;
        assert_eq!(STANDARD.decode(iv)?.len(), 12);
        assert_eq!(STANDARD.decode(tag)?.len(), 16);
        assert_eq!(fields["mac"], "");

        assert_eq!(
            service.decrypt_string(&wire)?,
            b"example application payload"
        );
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn example_cookie_round_trip_uses_the_default_laravel_provider() -> TestResult {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use openrust::encryption::LaravelEncrypter;

    let config = sources().merge(Serialized::defaults(json!({
        "app": {"cipher": "AES-128-CBC", "key": "0123456789abcdef"},
    })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&config)?)
        .build()
        .await?;
    let result: TestResult = async {
        let value = "Laravel + 雪";
        let response = post(&app, "queue", value).await?;
        success(&response, &json!({"action":"queue","queued":true}))?;
        let cookie = wire_cookie(&response)?;
        let escaped = cookie.replace('+', "%2B");
        let (_, wire) = url::form_urlencoded::parse(escaped.as_bytes())
            .next()
            .ok_or("Laravel cookie wire missing")?;
        let fields: Value = serde_json::from_slice(&STANDARD.decode(wire.as_bytes())?)?;
        assert_eq!(fields["tag"], "");
        assert_eq!(
            fields["mac"].as_str().ok_or("Laravel MAC missing")?.len(),
            64
        );
        let service = app.app()?.state().resolve::<LaravelEncrypter>().await?;
        let plaintext = service.decrypt_string(&wire)?;
        assert_eq!(plaintext.get(40), Some(&b'|'));
        assert_eq!(plaintext.get(41..), Some(value.as_bytes()));
        success(
            &get(&app, Some(&cookie)).await?,
            &json!({"preference":value,"present":true}),
        )?;
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn default_laravel_cookie_mode_requires_app_key_only_on_protected_request() -> TestResult {
    let config = sources().merge(Serialized::defaults(json!({
        "app": {"cipher": "AES-256-CBC", "key": "example-invalid-app-key"},
    })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&config)?)
        .build()
        .await?;
    let result: TestResult = async {
        let ordinary = app
            .request(
                Request::get("/api/example")
                    .header("host", "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        ordinary.assert_status(StatusCode::OK)?;
        let protected = get(&app, None).await?;
        protected.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
        assert!(!protected.headers().contains_key(header::SET_COOKIE));
        assert_eq!(protected.headers()[header::CACHE_CONTROL], "no-store");
        let payload: Value = protected.json()?;
        assert_eq!(payload["code"], "service_initialization_failed");
        assert!(!String::from_utf8_lossy(protected.body()).contains("example-invalid-app-key"));
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}
