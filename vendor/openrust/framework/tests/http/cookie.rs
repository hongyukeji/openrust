//! Cookie 快照、直接名称读取与既有响应组合的公开回归。
#![allow(clippy::expect_used)] // 测试 fixture 失败直接结束当前用例。
use axum::{
    body::Body,
    extract::{FromRequest, FromRequestParts},
    http::{Request as Wire, StatusCode},
    response::IntoResponse,
};
use openrust::http::{CookieFactory, CookieOptions, CookieResponseExt, Json, Request, RequestInfo};
use serde_json::json;

#[tokio::test]
async fn metadata_and_bodyowner_share_direct_cookie_semantics() {
    let wire = || {
        Wire::builder()
            .method("POST")
            .uri("/")
            .header(
                "cookie",
                "a.b=dot; a_b=second; empty=; tree[x]=nested; 0=numeric",
            )
            .header("content-type", "application/json")
            .body(Body::from("{\"body\":true}"))
            .expect("wire")
    };
    let (mut parts, _) = wire().into_parts();
    let info = RequestInfo::from_request_parts(&mut parts, &())
        .await
        .expect("info");
    assert_eq!(info.cookie("a_b"), Some(json!("dot")));
    assert_eq!(info.cookie("a.b"), None);
    assert_eq!(info.cookie("tree"), Some(json!({"x":"nested"})));
    assert_eq!(info.cookie("tree.x"), None);
    assert!(info.has_cookie("empty"));
    assert_eq!(
        info.cookie_or("missing", json!("default")),
        json!("default")
    );
    let request = Request::from_request(wire(), &()).await.expect("request");
    assert_eq!(request.cookies(), info.cookies());
    assert_eq!(request.input("body").expect("input"), Some(json!(true)));
    let (mut parts, _) = Wire::builder()
        .header("cookie", "0=zero; 1=one")
        .body(Body::empty())
        .expect("numeric wire")
        .into_parts();
    let numeric = RequestInfo::from_request_parts(&mut parts, &())
        .await
        .expect("numeric info");
    assert_eq!(numeric.cookie("0"), Some(json!("zero")));
    assert_eq!(numeric.cookie("00"), None);
}

#[test]
fn response_cookie_composition_preserves_envelope_and_unencoded_value() {
    let factory = CookieFactory::default();
    let cookie = factory
        .make("preference", "semi;plus+雪", 5, CookieOptions::default())
        .expect("cookie");
    let response = Json(json!({"ok":true}))
        .with_cookie(cookie)
        .expect("response")
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let header = response.headers()["set-cookie"].to_str().expect("header");
    assert!(header.starts_with("preference=semi%3Bplus%2B%E9%9B%AA;"));
    assert!(header.contains("httponly"));
    assert!(header.contains("samesite=lax"));
}

struct CookieRoutes;
impl openrust::support::ServiceProvider for CookieRoutes {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::new(tokio::sync::Notify::new()))
                .await?;
            Ok(())
        })
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.exceptions(|handler| {
                handler.api_envelope_when(|request, _| request.path().starts_with("/queue/"));
            })?;
            app.routes(|routes| {
                routes
                    .get("/without-cookies", || async { StatusCode::NO_CONTENT })
                    .group(
                        openrust::routing::RouteGroup::new().middleware_alias("cookies"),
                        |routes| {
                            routes
                                .get("/queue/{value}", scoped_queue)
                                .get("/inspect", empty_queue)
                                .get("/cancel", cancel_queue)
                                .get("/security", security_cookies)
                        },
                    )
            })?;
            Ok(())
        })
    }
}
async fn scoped_queue(
    jar: openrust::http::CookieJar,
    openrust::http::Inject(other): openrust::http::Inject<openrust::http::CookieJar>,
    axum::extract::Path(value): axum::extract::Path<String>,
) -> openrust::Result<Json<serde_json::Value>> {
    jar.queue(
        jar.factory()
            .make("scoped", value, 0, CookieOptions::default())?,
    )?;
    Ok(Json(
        json!({"same_scope":other.has_queued("scoped", None)?}),
    ))
}
async fn empty_queue(jar: openrust::http::CookieJar) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(json!({"empty":jar.get_queued_cookies()?.is_empty()})))
}
async fn cancel_queue(
    jar: openrust::http::CookieJar,
    openrust::http::Inject(ready): openrust::http::Inject<tokio::sync::Notify>,
) -> openrust::Result<()> {
    jar.queue(
        jar.factory()
            .make("cancelled", "never_send", 0, CookieOptions::default())?,
    )?;
    ready.notify_one();
    std::future::pending().await
}
async fn security_cookies(jar: openrust::http::CookieJar) -> openrust::Result<impl IntoResponse> {
    let cookies = [
        ("automatic", None),
        ("forced", Some(true)),
        ("disabled", Some(false)),
    ]
    .into_iter()
    .map(|(name, secure)| {
        jar.factory().make(
            name,
            "value",
            0,
            CookieOptions {
                secure,
                ..CookieOptions::default()
            },
        )
    })
    .collect::<openrust::Result<Vec<_>>>()?;
    Json(json!({"ok":true})).with_cookies(cookies)
}
async fn cookie_app(
    provider: openrust::http::CookieServiceProvider,
) -> openrust::Result<openrust::Application> {
    let mut config = openrust::config::FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    config.server.trusted_proxy_ranges = Some(vec!["127.0.0.1/32".to_owned()]);
    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder
        .provider(openrust::http::HttpServiceProvider)
        .provider(provider)
        .provider(CookieRoutes);
    openrust::Application::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(config),
            ))
            .await?,
    )
}
#[tokio::test]
async fn request_scope_shares_jar_but_concurrent_and_cancelled_requests_are_isolated()
-> openrust::Result<()> {
    use openrust::ServiceExt;
    let app = cookie_app(openrust::http::CookieServiceProvider::plain()).await?;
    let router = openrust::foundation::http::build_router(&app)?;
    let send = |path: &str| {
        Wire::builder()
            .uri(path)
            .header("host", "localhost")
            .body(Body::empty())
            .expect("wire")
    };
    let (one, two) = tokio::join!(
        router.clone().oneshot(send("/queue/one")),
        router.clone().oneshot(send("/queue/two"))
    );
    for (response, value) in [(one.expect("one"), "one"), (two.expect("two"), "two")] {
        assert!(
            response.headers()["set-cookie"]
                .to_str()
                .expect("cookie")
                .starts_with(&format!("scoped={value};"))
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .map_err(openrust::Error::internal)?;
        let value: serde_json::Value =
            serde_json::from_slice(&body).map_err(openrust::Error::internal)?;
        assert_eq!(value["data"]["same_scope"], true);
    }
    let ready = app
        .state()
        .container()
        .resolve::<tokio::sync::Notify>()
        .await?;
    let cancelled = tokio::spawn(router.clone().oneshot(send("/cancel")));
    tokio::time::timeout(std::time::Duration::from_secs(2), ready.notified())
        .await
        .map_err(openrust::Error::internal)?;
    cancelled.abort();
    assert!(cancelled.await.expect_err("cancelled").is_cancelled());
    let response = router.oneshot(send("/inspect")).await.expect("inspect");
    assert!(!response.headers().contains_key("set-cookie"));
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(openrust::Error::internal)?;
    assert_eq!(value["data"]["empty"], true);
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    Ok(())
}

#[tokio::test]
async fn protected_provider_never_implicitly_downgrades_to_plain() -> openrust::Result<()> {
    use openrust::ServiceExt;

    let app = cookie_app(openrust::http::CookieServiceProvider::new()).await?;
    let router = openrust::foundation::http::build_router(&app)?;
    let send = |path: &str| {
        Wire::builder()
            .uri(path)
            .header("host", "localhost")
            .body(Body::empty())
            .expect("wire")
    };
    let ordinary = router
        .clone()
        .oneshot(send("/without-cookies"))
        .await
        .expect("ordinary response");
    assert_eq!(ordinary.status(), StatusCode::NO_CONTENT);
    assert!(!ordinary.headers().contains_key("set-cookie"));

    let protected = router
        .oneshot(send("/queue/protected"))
        .await
        .expect("protected response");
    let (status, code) = if cfg!(feature = "encryption") {
        (StatusCode::INTERNAL_SERVER_ERROR, "service_not_bound")
    } else {
        (StatusCode::BAD_REQUEST, "cookie_encryption_unavailable")
    };
    assert_eq!(protected.status(), status);
    assert!(!protected.headers().contains_key("set-cookie"));
    assert_eq!(protected.headers()["cache-control"], "no-store");
    let body = axum::body::to_bytes(protected.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(openrust::Error::internal)?;
    assert_eq!(value["code"], code);
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn router_snapshot_keeps_cookie_plan_after_runtime_provider_replacement()
-> openrust::Result<()> {
    use openrust::ServiceExt;

    let mut app = cookie_app(openrust::http::CookieServiceProvider::plain()).await?;
    let old = openrust::foundation::http::build_router(&app)?;
    app.force_register_provider(openrust::http::CookieServiceProvider::new())
        .await?;
    let current = openrust::foundation::http::build_router(&app)?;
    let request = || {
        Wire::builder()
            .uri("/queue/value")
            .header("host", "localhost")
            .body(Body::empty())
            .expect("wire")
    };

    let original = old
        .oneshot(request())
        .await
        .expect("original plan response");
    assert_eq!(original.status(), StatusCode::OK);
    assert!(
        original.headers()["set-cookie"]
            .to_str()
            .expect("original plain cookie")
            .starts_with("scoped=value;")
    );
    let replacement = current
        .oneshot(request())
        .await
        .expect("replacement plan response");
    let expected = if cfg!(feature = "encryption") {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::BAD_REQUEST
    };
    assert_eq!(replacement.status(), expected);
    assert!(!replacement.headers().contains_key("set-cookie"));
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await
}

#[tokio::test]
async fn origin_form_secure_defaults_use_trusted_transport_and_preserve_explicit_options()
-> openrust::Result<()> {
    use axum::extract::connect_info::ConnectInfo;
    use openrust::ServiceExt;
    use std::net::SocketAddr;
    let app = cookie_app(openrust::http::CookieServiceProvider::plain()).await?;
    let router = openrust::foundation::http::build_router(&app)?;
    for (peer, proto, native_tls, expected) in [
        ("127.0.0.1:80", "https", None, true),
        ("192.0.2.1:80", "https", None, false),
        ("127.0.0.1:80", "ssl", None, true),
        ("127.0.0.1:80", " HTTPS , http", None, true),
        ("127.0.0.1:80", "http, https", None, false),
        ("127.0.0.1:80", "on", None, true),
        ("127.0.0.1:80", "1", None, true),
        ("127.0.0.1:80", "unknown", None, false),
        ("192.0.2.1:80", "http", Some(true), true),
        ("127.0.0.1:80", "https", Some(false), true),
    ] {
        let mut request = Wire::builder()
            .uri("/security")
            .header("host", "localhost")
            .header("x-forwarded-proto", proto)
            .body(Body::empty())
            .expect("wire");
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().expect("peer")));
        if let Some(secure) = native_tls {
            request
                .extensions_mut()
                .insert(openrust::http::RequestSecurity::new(secure));
        }
        let response = router.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        for (name, expected) in [
            ("automatic", expected),
            ("forced", true),
            ("disabled", false),
        ] {
            let wire = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .find(|header| header.as_bytes().starts_with(format!("{name}=").as_bytes()))
                .expect("cookie")
                .to_str()
                .expect("header");
            assert_eq!(
                wire.split(';')
                    .any(|part| part.trim().eq_ignore_ascii_case("secure")),
                expected,
                "{name} peer={peer} proto={proto}"
            );
        }
    }
    let mut request = Wire::builder()
        .uri("/security")
        .header("host", "localhost")
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-proto", "http")
        .body(Body::empty())
        .expect("ambiguous wire");
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:80".parse::<SocketAddr>().expect("peer"),
    ));
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let automatic = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find(|header| header.as_bytes().starts_with(b"automatic="))
        .expect("automatic");
    assert!(automatic.to_str().expect("header").contains("; secure"));
    // 客户端提供 absolute-form https 不能代替原生连接的 TLS 属性。
    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Wire::builder()
                .uri("https://localhost/security")
                .header("host", "localhost")
                .body(Body::empty())
                .expect("absolute wire"),
        )
        .await
        .expect("response");
    let automatic = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find(|header| header.as_bytes().starts_with(b"automatic="))
        .expect("automatic");
    assert!(!automatic.to_str().expect("header").contains("; secure"));
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    Ok(())
}

struct LateSessionDefaults;
impl openrust::support::ServiceProvider for LateSessionDefaults {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.overlay("session", json!({"path":"/late", "domain":"example.test", "secure":false, "same_site":"strict"}))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn factory_uses_final_session_section_without_auth_and_explicit_factory_wins()
-> openrust::Result<()> {
    for explicit in [false, true] {
        let mut factory = CookieFactory::default();
        factory.set_default_path_and_domain(
            "/explicit".to_owned(),
            None,
            Some(true),
            Some(openrust::http::SameSite::Lax),
        )?;
        let provider = if explicit {
            openrust::http::CookieServiceProvider::plain().with_factory(factory)
        } else {
            openrust::http::CookieServiceProvider::plain()
        };
        let mut builder = openrust::foundation::ProviderBuilder::new();
        builder.provider(provider).provider(LateSessionDefaults);
        let runtime = builder.start(figment::Figment::new()).await?;
        let factory = runtime.container().resolve::<CookieFactory>().await?;
        let cookie = factory.make("example", "value", 0, CookieOptions::default())?;
        assert_eq!(cookie.path(), if explicit { "/explicit" } else { "/late" });
        assert_eq!(
            cookie.domain(),
            if explicit { None } else { Some("example.test") }
        );
        assert_eq!(cookie.secure(), Some(explicit));
        assert_eq!(
            cookie.same_site(),
            Some(if explicit {
                openrust::http::SameSite::Lax
            } else {
                openrust::http::SameSite::Strict
            })
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await?;
    }
    Ok(())
}

struct EarlyCookieFactoryProbe(
    std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<CookieFactory>>>>,
);
impl openrust::support::ServiceProvider for EarlyCookieFactoryProbe {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let early = app
                .rebinding::<CookieFactory, _>(|_, _| Box::pin(async { Ok(()) }))
                .await?
                .expect("Cookie factory is bound before probe registration");
            *self.0.lock().await = Some(early);
            Ok(())
        })
    }
}

#[tokio::test]
async fn early_cookie_factory_resolution_keeps_singleton_snapshot_after_late_session_overlay()
-> openrust::Result<()> {
    for explicit in [false, true] {
        let mut supplied = CookieFactory::default();
        supplied.set_default_path_and_domain(
            "/explicit".to_owned(),
            None,
            Some(true),
            Some(openrust::http::SameSite::Lax),
        )?;
        let provider = if explicit {
            openrust::http::CookieServiceProvider::plain().with_factory(supplied)
        } else {
            openrust::http::CookieServiceProvider::plain()
        };
        let early = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let mut builder = openrust::foundation::ProviderBuilder::new();
        builder
            .provider(provider)
            .provider(EarlyCookieFactoryProbe(early.clone()))
            .provider(LateSessionDefaults);
        let runtime = builder.start(figment::Figment::new()).await?;
        let before_boot = early.lock().await.clone().expect("early factory");
        let after_boot = runtime.container().resolve::<CookieFactory>().await?;
        assert!(std::sync::Arc::ptr_eq(&before_boot, &after_boot));
        for factory in [&before_boot, &after_boot] {
            let cookie = factory.make("example", "value", 0, CookieOptions::default())?;
            assert_eq!(cookie.path(), if explicit { "/explicit" } else { "/" });
            assert_eq!(cookie.domain(), None);
            assert_eq!(cookie.secure(), if explicit { Some(true) } else { None });
            assert_eq!(cookie.same_site(), Some(openrust::http::SameSite::Lax));
        }
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn existing_session_section_preserves_null_or_missing_same_site_without_auth_provider()
-> openrust::Result<()> {
    for (section, expected) in [
        (None, Some(openrust::http::SameSite::Lax)),
        (Some(json!({})), None),
        (Some(json!({"same_site":null})), None),
        (
            Some(json!({"same_site":"strict"})),
            Some(openrust::http::SameSite::Strict),
        ),
    ] {
        let source = match section {
            Some(section) => figment::Figment::new().merge(("session", section)),
            None => figment::Figment::new(),
        };
        let mut builder = openrust::foundation::ProviderBuilder::new();
        builder.provider(openrust::http::CookieServiceProvider::plain());
        let runtime = builder.start(source).await?;
        let factory = runtime.container().resolve::<CookieFactory>().await?;
        assert_eq!(
            factory
                .make("example", "value", 0, CookieOptions::default())?
                .same_site(),
            expected
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
            .await?;
    }
    Ok(())
}

#[test]
fn empty_and_absent_domains_share_response_identity_and_preserve_path_name_order()
-> openrust::Result<()> {
    let factory = CookieFactory::default();
    let mut empty_domain = CookieFactory::default();
    empty_domain.set_default_path_and_domain(
        "/".to_owned(),
        Some(String::new()),
        None,
        Some(openrust::http::SameSite::Lax),
    )?;
    let response = Json(json!({"ok":true}))
        .with_cookies([
            factory.make("first", "original", 0, CookieOptions::default())?,
            factory.make(
                "other_path",
                "value",
                0,
                CookieOptions {
                    path: Some("/other".to_owned()),
                    ..CookieOptions::default()
                },
            )?,
            empty_domain.make("second", "value", 0, CookieOptions::default())?,
            empty_domain.make("first", "replacement", 0, CookieOptions::default())?,
        ])?
        .with_cookie(openrust::http::Cookie::from_header(
            "first=from-header; path=/; domain=",
        )?)?
        .into_response();
    let headers = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| std::str::from_utf8(header.as_bytes()).map_err(openrust::Error::internal))
        .collect::<openrust::Result<Vec<_>>>()?;
    assert_eq!(headers.len(), 3);
    for (header, prefix) in
        headers
            .iter()
            .zip(["first=from-header;", "second=value;", "other_path=value;"])
    {
        assert!(header.starts_with(prefix));
        assert!(!header.contains("domain="));
    }
    Ok(())
}

struct ReplaceWithProtectedCookies;

impl openrust::support::ServiceProvider for ReplaceWithProtectedCookies {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::foundation::RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let mut factory = CookieFactory::default();
            factory.set_default_path_and_domain("/child".into(), None, None, None)?;
            app.force_register_provider(
                openrust::http::CookieServiceProvider::new().with_factory(factory),
            )
            .await?;
            let factory = app.resolve::<CookieFactory>().await?;
            assert_eq!(
                factory
                    .make("probe", "value", 0, CookieOptions::default())?
                    .path(),
                "/child"
            );
            Ok(())
        })
    }
}

#[tokio::test]
async fn startup_forced_registration_uses_final_cookie_plan_and_factory() -> openrust::Result<()> {
    use openrust::ServiceExt;

    let mut factory = CookieFactory::default();
    factory.set_default_path_and_domain("/final".into(), None, None, None)?;
    let mut providers = openrust::foundation::ProviderBuilder::new();
    providers
        .provider(openrust::http::HttpServiceProvider)
        .provider(openrust::http::CookieServiceProvider::plain())
        .provider(ReplaceWithProtectedCookies)
        .force_provider(openrust::http::CookieServiceProvider::plain().with_factory(factory))
        .provider(CookieRoutes);
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ));
    let app = openrust::Application::from_provider_runtime(providers.start(sources).await?)?;
    let response = openrust::foundation::http::build_router(&app)?
        .oneshot(
            Wire::builder()
                .uri("/queue/value")
                .header("host", "localhost")
                .body(Body::empty())
                .expect("wire"),
        )
        .await
        .expect("final Cookie provider response");
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .expect("final plan uses plain Cookies");
    assert!(cookie.starts_with("scoped=value;"));
    assert!(
        cookie
            .split(';')
            .any(|attribute| attribute.trim() == "path=/final")
    );
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn malformed_key_fails_at_first_protected_request_without_leaking_material()
-> openrust::Result<()> {
    use openrust::ServiceExt;

    let mut providers = openrust::foundation::ProviderBuilder::new();
    providers
        .provider(openrust::http::HttpServiceProvider)
        .provider(openrust::encryption::EncryptionServiceProvider)
        .provider(openrust::http::CookieServiceProvider::new())
        .provider(CookieRoutes);
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge((
        "security",
        json!({"encryption":{"active_key_id":"test","keys":{"test":"invalid-key-sentinel"}}}),
    ));
    let app = openrust::Application::from_provider_runtime(providers.start(sources).await?)?;
    let router = openrust::foundation::http::build_router(&app)?;
    let request = |uri| {
        Wire::builder()
            .uri(uri)
            .header("host", "localhost")
            .body(Body::empty())
            .expect("wire")
    };
    let ordinary = router
        .clone()
        .oneshot(request("/without-cookies"))
        .await
        .expect("ordinary response");
    assert_eq!(ordinary.status(), StatusCode::NO_CONTENT);
    let response = router
        .oneshot(request("/queue/protected"))
        .await
        .expect("rejected protected response");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!response.headers().contains_key("set-cookie"));
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(openrust::Error::internal)?;
    assert!(!String::from_utf8_lossy(&bytes).contains("invalid-key-sentinel"));
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(openrust::Error::internal)?;
    assert_eq!(value["code"], "service_initialization_failed");
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await
}
