//! Signed URL 从配置轮换到原始查询、中间件和 Redirector 的公开调用链。

use super::*;
use futures_util::FutureExt as _;
use openrust::http::{Redirector, RequestInfo};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct GlobalExcept;
impl openrust::core::ServiceProvider for GlobalExcept {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.signed_url_except(["global"])?;
            Ok(())
        })
    }
}

async fn issue(urls: RouteUrlGenerator) -> openrust::Result<Json<serde_json::Value>> {
    let params = BTreeMap::from([
        ("id".to_owned(), "42".to_owned()),
        ("kind".to_owned(), "draft".to_owned()),
        ("sort".to_owned(), "recent".to_owned()),
    ]);
    Ok(Json(serde_json::json!({
        "absolute":urls.signed_route("signed.show", &params, true)?,
        "relative":urls.signed_route("signed.show", &params, false)?,
        "temporary":urls.temporary_signed_route("signed.show", 120, &params, true)?,
        "ignored":urls.signed_route("signed.ignored", &params, true)?,
        "guarded":urls.signed_route("signed.guarded", &params, true)?,
        "relative_guarded":urls.signed_route("signed.relative", &params, false)?,
        "expired_guarded":urls.signed_route_at("signed.guarded", &params, Some(1), true)?,
    })))
}

async fn show(info: RequestInfo) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"valid":info.has_valid_signature()?, "relative":info.has_valid_relative_signature()?, "ignoring":info.has_valid_signature_while_ignoring(&["tracking"], true)?}),
    ))
}

async fn redirect(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info)
        .signed_route(
            "signed.show",
            &BTreeMap::from([("id".to_owned(), "42".to_owned())]),
        )?
        .into_response())
}

async fn fixture_with_except(
    except: bool,
) -> Result<(openrust::App, axum::Router, Arc<AtomicUsize>), Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.app.url = "http://signed.example".into();
    config.app.key = Some("base64:YWJj".into());
    let mut builder = openrust::core::ProviderBuilder::new();
    let calls = Arc::new(AtomicUsize::new(0));
    if except {
        builder.provider(GlobalExcept);
    }
    let counter = Arc::clone(&calls);
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(move |routes| {
            let guarded_calls = Arc::clone(&counter);
            let relative_calls = Arc::clone(&counter);
            routes
                .get("/issue", issue)
                .get("/redirect", redirect)
                .get("/documents/{id}", show)
                .name("signed.show")
                .group(RouteGroup::new(), move |signed| {
                    signed
                        .get("/guarded/{id}", move || {
                            let calls = Arc::clone(&guarded_calls);
                            async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                "accepted"
                            }
                        })
                        .name("signed.guarded")
                        .middleware_alias("signed")
                })
                .group(RouteGroup::new(), move |signed| {
                    signed
                        .get("/relative/{id}", move || {
                            let calls = Arc::clone(&relative_calls);
                            async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                "accepted"
                            }
                        })
                        .name("signed.relative")
                        .middleware_alias("signed:relative")
                })
                .group(RouteGroup::new(), |signed| {
                    signed
                        .get("/ignored/{id}", show)
                        .name("signed.ignored")
                        .middleware_alias("signed:tracking")
                })
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(config),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    Ok((app, router, calls))
}

async fn fixture()
-> Result<(openrust::App, axum::Router, Arc<AtomicUsize>), Box<dyn std::error::Error>> {
    fixture_with_except(false).await
}

async fn get(router: &axum::Router, uri: &str) -> TestResultResponse {
    let response = router
        .clone()
        .oneshot(
            Request::get(uri)
                .header(header::HOST, "signed.example")
                .body(Body::empty())?,
        )
        .await?;
    Ok(response)
}

type TestResultResponse = Result<Response, Box<dyn std::error::Error>>;

async fn issued(router: &axum::Router) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let response = get(router, "/issue").await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 8192).await?;
    if status != StatusCode::OK {
        return Err(format!(
            "签名 URL 签发失败 {status}: {}",
            String::from_utf8_lossy(&body)
        )
        .into());
    }
    Ok(serde_json::from_slice(&body)?)
}

#[tokio::test]
async fn signed_urls_use_raw_query_and_live_key_rotation() -> TestResult {
    let (app, router, _) = fixture().await?;
    let body = issued(&router).await?;
    let data = &body["data"];
    let absolute = data["absolute"].as_str().ok_or("missing absolute URL")?;
    let path = absolute
        .strip_prefix("http://signed.example")
        .ok_or("wrong origin")?;
    let valid = get(&router, path).await?;
    assert_eq!(valid.status(), StatusCode::OK);
    let valid: serde_json::Value =
        serde_json::from_slice(&to_bytes(valid.into_body(), 8192).await?)?;
    assert_eq!(valid["data"]["valid"], true);
    let tampered = get(&router, &path.replace("kind=draft", "kind=published")).await?;
    let tampered: serde_json::Value =
        serde_json::from_slice(&to_bytes(tampered.into_body(), 8192).await?)?;
    assert_eq!(tampered["data"]["valid"], false);
    let with_tracking = get(&router, &format!("{path}&tracking=one")).await?;
    let with_tracking: serde_json::Value =
        serde_json::from_slice(&to_bytes(with_tracking.into_body(), 8192).await?)?;
    assert_eq!(with_tracking["data"]["valid"], false);
    assert_eq!(with_tracking["data"]["ignoring"], true);
    let encoded_tracking = get(&router, &format!("{path}&%74racking=one")).await?;
    let encoded_tracking: serde_json::Value =
        serde_json::from_slice(&to_bytes(encoded_tracking.into_body(), 8192).await?)?;
    assert_eq!(encoded_tracking["data"]["ignoring"], false);
    let reordered = path.replace("kind=draft&sort=recent", "sort=recent&kind=draft");
    let reordered = get(&router, &reordered).await?;
    let reordered: serde_json::Value =
        serde_json::from_slice(&to_bytes(reordered.into_body(), 8192).await?)?;
    assert_eq!(reordered["data"]["valid"], false);
    let ignored = data["ignored"].as_str().ok_or("missing ignored URL")?;
    let ignored = ignored
        .strip_prefix("http://signed.example")
        .ok_or("wrong ignored origin")?;
    let ignored_with_tracking = get(&router, &format!("{ignored}&tracking=one")).await?;
    assert_eq!(ignored_with_tracking.status(), StatusCode::OK);
    let ignored_with_tracking: serde_json::Value =
        serde_json::from_slice(&to_bytes(ignored_with_tracking.into_body(), 8192).await?)?;
    assert_eq!(ignored_with_tracking["data"]["ignoring"], true);
    let repository = app.config_repository();
    repository.set("app.key", serde_json::json!("next"))?;
    let old = get(&router, path).await?;
    let old: serde_json::Value = serde_json::from_slice(&to_bytes(old.into_body(), 8192).await?)?;
    assert_eq!(old["data"]["valid"], false);
    repository.set("app.previous_keys", serde_json::json!(["base64:YWJj"]))?;
    let old = get(&router, path).await?;
    let old: serde_json::Value = serde_json::from_slice(&to_bytes(old.into_body(), 8192).await?)?;
    assert_eq!(old["data"]["valid"], true);
    repository.set("app.key", serde_json::Value::Null)?;
    let null_key = get(&router, "/issue").await?;
    let null_key: serde_json::Value =
        serde_json::from_slice(&to_bytes(null_key.into_body(), 8192).await?)?;
    repository.set("app.key", serde_json::json!(""))?;
    let empty_key = get(&router, "/issue").await?;
    let empty_key: serde_json::Value =
        serde_json::from_slice(&to_bytes(empty_key.into_body(), 8192).await?)?;
    assert_eq!(null_key["data"]["absolute"], empty_key["data"]["absolute"]);
    repository.set("app.key", serde_json::json!("0"))?;
    let zero_signed = get(&router, "/issue").await?;
    let zero_signed: serde_json::Value =
        serde_json::from_slice(&to_bytes(zero_signed.into_body(), 8192).await?)?;
    let zero_path = zero_signed["data"]["absolute"]
        .as_str()
        .ok_or("zero key URL 缺失")?
        .strip_prefix("http://signed.example")
        .ok_or("zero key origin 错误")?
        .to_owned();
    repository.set("app.key", serde_json::json!("active"))?;
    repository.set("app.previous_keys", serde_json::json!([0]))?;
    let zero_valid = get(&router, &zero_path).await?;
    let zero_valid: serde_json::Value =
        serde_json::from_slice(&to_bytes(zero_valid.into_body(), 8192).await?)?;
    assert_eq!(zero_valid["data"]["valid"], true);
    Ok(())
}

#[tokio::test]
async fn signed_middleware_rejects_invalid_request_before_handler() -> TestResult {
    let (_, router, calls) = fixture().await?;
    assert_eq!(
        get(&router, "/guarded/42?signature=bad").await?.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&router, "/relative/42?signature=bad").await?.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let issue = issued(&router).await?;
    for key in ["guarded", "relative_guarded"] {
        let target = issue["data"][key].as_str().ok_or("有效签名链接缺失")?;
        let path = target
            .strip_prefix("http://signed.example")
            .unwrap_or(target);
        assert_eq!(get(&router, path).await?.status(), StatusCode::OK);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let expired = issue["data"]["expired_guarded"]
        .as_str()
        .ok_or("过期链接缺失")?;
    let expired = expired
        .strip_prefix("http://signed.example")
        .ok_or("过期链接 origin 错误")?;
    assert_eq!(get(&router, expired).await?.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let response = get(&router, "/redirect").await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert!(
        response.headers()[header::LOCATION]
            .to_str()?
            .contains("signature=")
    );
    Ok(())
}

#[tokio::test]
async fn signed_global_except_applies_to_default_and_parameterized_aliases_per_app() -> TestResult {
    let (_, accepting, accepted_calls) = fixture_with_except(true).await?;
    let (_, rejecting, rejected_calls) = fixture_with_except(false).await?;
    let issue = issued(&accepting).await?;
    for (key, extra) in [
        ("guarded", "&global=one"),
        ("relative_guarded", "&global=two"),
        ("ignored", "&tracking=one&global=three"),
    ] {
        let target = issue["data"][key].as_str().ok_or("签名链接缺失")?;
        let path = target
            .strip_prefix("http://signed.example")
            .unwrap_or(target);
        let path = format!("{path}{extra}");
        assert_eq!(get(&accepting, &path).await?.status(), StatusCode::OK);
        assert_eq!(
            get(&rejecting, &path).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(accepted_calls.load(Ordering::SeqCst), 2);
    assert_eq!(rejected_calls.load(Ordering::SeqCst), 0);
    Ok(())
}

async fn tcp_get(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    path: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client
        .get(format!("http://{address}{path}"))
        .header(reqwest::header::HOST, "signed.example")
        .send()
        .await
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实 TCP 连接生命周期覆盖签名家族和 handler 副作用边界"
)]
async fn signed_urls_over_real_tcp_enforce_expiry_ignore_rotation_and_handler_boundary()
-> TestResult {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (app, router, calls) = fixture_with_except(true).await?;
    eprintln!("signed URL TCP loopback listener: {address}");
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        std::panic::AssertUnwindSafe(async {
            let issued: serde_json::Value =
                tcp_get(&client, address, "/issue").await?.json().await?;
            let data = &issued["data"];
            let path = data["guarded"]
                .as_str()
                .ok_or("TCP signed URL 缺失")?
                .strip_prefix("http://signed.example")
                .ok_or("TCP signed URL origin 错误")?;
            assert_eq!(
                tcp_get(&client, address, path).await?.status().as_u16(),
                200
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                tcp_get(
                    &client,
                    address,
                    &path.replace("kind=draft", "kind=changed")
                )
                .await?
                .status()
                .as_u16(),
                403
            );
            let expired = data["expired_guarded"]
                .as_str()
                .ok_or("TCP expired URL 缺失")?
                .strip_prefix("http://signed.example")
                .ok_or("TCP expired URL origin 错误")?;
            assert_eq!(
                tcp_get(&client, address, expired).await?.status().as_u16(),
                403
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let relative = data["relative_guarded"]
                .as_str()
                .ok_or("TCP relative URL 缺失")?;
            assert_eq!(
                tcp_get(&client, address, relative).await?.status().as_u16(),
                200
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert_eq!(
                tcp_get(&client, address, &format!("{path}&global=one"))
                    .await?
                    .status()
                    .as_u16(),
                200
            );
            let ignored = data["ignored"]
                .as_str()
                .ok_or("TCP ignored URL 缺失")?
                .strip_prefix("http://signed.example")
                .ok_or("TCP ignored URL origin 错误")?;
            assert_eq!(
                tcp_get(
                    &client,
                    address,
                    &format!("{ignored}&tracking=one&global=two")
                )
                .await?
                .status()
                .as_u16(),
                200
            );
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            let repository = app.config_repository();
            repository.set("app.key", serde_json::json!("next"))?;
            assert_eq!(
                tcp_get(&client, address, path).await?.status().as_u16(),
                403
            );
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            repository.set("app.previous_keys", serde_json::json!(["base64:YWJj"]))?;
            assert_eq!(
                tcp_get(&client, address, path).await?.status().as_u16(),
                200
            );
            assert_eq!(calls.load(Ordering::SeqCst), 4);
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .catch_unwind(),
    )
    .await;
    let _ = stop.send(());
    let server_cleanup =
        match tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(format!("TCP server shutdown failed: {error}")),
            Ok(Err(error)) => Err(format!("TCP server join failed: {error}")),
            Err(_) => {
                server.abort();
                match tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
                    Ok(_) => Err("TCP server graceful shutdown timed out; task aborted".to_owned()),
                    Err(_) => Err("TCP server abort join timed out".to_owned()),
                }
            }
        };
    let app_cleanup = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5)),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("TCP App shutdown failed: {error}")),
        Err(_) => Err("TCP App shutdown timed out".to_owned()),
    };
    let port_cleanup = if server_cleanup.is_ok() {
        tokio::net::TcpListener::bind(address)
            .await
            .map(drop)
            .map_err(|error| format!("TCP port was not released: {error}"))
    } else {
        Ok(())
    };
    let cleanup = server_cleanup.and(app_cleanup).and(port_cleanup);
    match result {
        Ok(Ok(Ok(()))) => cleanup.map_err(Into::into),
        Ok(Ok(Err(error))) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!("TCP cleanup after request failure: {cleanup_error}");
            }
            Err(error)
        }
        Ok(Err(panic)) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!("TCP cleanup after assertion panic: {cleanup_error}");
            }
            std::panic::resume_unwind(panic)
        }
        Err(error) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!("TCP cleanup after total timeout: {cleanup_error}");
            }
            Err(Box::<dyn std::error::Error>::from(error))
        }
    }
}
