//! 通过 `tower::ServiceExt::oneshot` 验证 HTTP 层的端到端行为。

use axum::body::{Body, to_bytes};
use axum::extract::{Extension, Request};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt as _;
use http_body::Body as _;
use openrust::ExecutionContext;
use openrust::config::FrameworkConfig;
use openrust::core::container::Scope;
use openrust::core::{RateLimitBucket, RateLimitFuture, RateLimitStore, RateLimiter};
use openrust::http::RateLimitLayer;
use openrust::http::axum;
use openrust::http::{
    Created, DomainParams, HttpServiceProvider, Inject, NoContent, PageParams, Paginated,
    RouteGroup, RouteUrlGenerator, Routes, Validated, build_router,
};
use openrust::http::{Json, Path, Query};

use openrust::validation::{Rule, Rules, Validate};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[path = "router/action_redirect.rs"]
mod action_redirect;
#[path = "router/collection_dispatch.rs"]
mod collection_dispatch;
#[path = "router/signed_url.rs"]
mod signed_url;

#[derive(Deserialize)]
struct DomainTenant {
    account: String,
}

async fn domain_order(
    DomainParams(tenant): DomainParams<DomainTenant>,
    Path(id): Path<String>,
    request: Request,
) -> String {
    let uri = request.uri().to_string();
    let header = request
        .headers()
        .get("x-domain-proof")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing")
        .to_owned();
    let body = to_bytes(request.into_body(), 4096)
        .await
        .unwrap_or_default();
    format!(
        "{}:{id}:{uri}:{header}:{}",
        tenant.account,
        String::from_utf8_lossy(&body)
    )
}

async fn root_order(request: Request) -> String {
    let uri = request.uri().to_string();
    let body = to_bytes(request.into_body(), 4096)
        .await
        .unwrap_or_default();
    format!("root:{uri}:{}", String::from_utf8_lossy(&body))
}

async fn terminal_domain_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn domain_fallback_marker() -> &'static str {
    "domain fallback"
}
async fn root_fallback_marker() -> &'static str {
    "root fallback"
}
async fn first_domain_marker() -> &'static str {
    "first domain"
}
async fn second_domain_marker() -> &'static str {
    "second domain"
}

async fn merged_named_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let parameters = std::collections::BTreeMap::from([
        ("account".into(), "Acme".into()),
        ("id".into(), "42".into()),
    ]);
    Ok(Json(urls.route("merged.show", &parameters)?))
}

async fn merged_explicit_named_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    Ok(Json(urls.route(
        "explicit.named",
        &std::collections::BTreeMap::new(),
    )?))
}

async fn domain_named_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let parameters = std::collections::BTreeMap::from([
        ("account".into(), "Acme".into()),
        ("id".into(), "a b/c".into()),
        ("page".into(), "2".into()),
    ]);
    Ok(Json(urls.route("orders.show", &parameters)?))
}

async fn domain_named_url_invalid(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let parameters = std::collections::BTreeMap::from([
        ("account".into(), "a.b".into()),
        ("id".into(), "1".into()),
    ]);
    Ok(Json(urls.route("orders.show", &parameters)?))
}

async fn domain_test_router() -> Result<axum::Router, Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.app.url = "https://app.example.test:8443/base".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .post("/orders/{id}", root_order)
                .get("/named-domain", domain_named_url)
                .get("/named-domain-invalid", domain_named_url_invalid)
                .fallback(fallback_echo)
                .group(
                    RouteGroup::new().domain("{account}.example.test"),
                    |domain| {
                        domain
                            .get("/orders/{id}", domain_order)
                            .name("orders.show")
                            .get("/terminal", terminal_domain_not_found)
                            .fallback(fallback_echo)
                    },
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    Ok(build_router(&app)?)
}

#[tokio::test]
async fn domain_routes_precede_root_and_keep_body_for_method_fallback() -> TestResult {
    let router = domain_test_router().await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/orders/7?q=1")
                .header(header::HOST, "ACME.example.test:8443")
                .header("x-domain-proof", "preserved")
                .body(Body::from("get body"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await?,
        "acme:7:/orders/7?q=1:preserved:get body"
    );

    let response = router
        .clone()
        .oneshot(
            Request::post("/orders/7?q=2")
                .header(header::HOST, "acme.example.test")
                .body(Body::from("post body"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await?,
        "root:/orders/7?q=2:post body"
    );

    let streamed = Body::from_stream(futures_util::stream::iter([
        Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"stream ")),
        Ok(bytes::Bytes::from_static(b"body")),
    ]));
    let response = router
        .clone()
        .oneshot(
            Request::post("/orders/8?stream=1")
                .header(header::HOST, "acme.example.test")
                .body(streamed)?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await?,
        "root:/orders/8?stream=1:stream body"
    );

    let response = router
        .clone()
        .oneshot(
            Request::put("/orders/7")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let allow = response
        .headers()
        .get(header::ALLOW)
        .ok_or("missing Allow")?
        .to_str()?;
    assert!(allow.contains("GET") && allow.contains("POST"), "{allow}");

    let terminal = router
        .clone()
        .oneshot(
            Request::get("/terminal")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(terminal.status(), StatusCode::NOT_FOUND);
    let missing = router
        .clone()
        .oneshot(
            Request::get("/unknown")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(missing.into_body(), 4096).await?,
        "/unknown|missing|"
    );
    Ok(())
}

#[tokio::test]
async fn domain_group_middleware_runs_only_for_matched_handler() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(move |routes| {
            let counter = Arc::clone(&counter);
            routes.post("/shared", root_order).group(
                RouteGroup::new()
                    .domain("{account}.example.test")
                    .middleware(axum::middleware::from_fn(
                        move |request: Request, next: Next| {
                            let counter = Arc::clone(&counter);
                            async move {
                                counter.fetch_add(1, Ordering::SeqCst);
                                next.run(request).await
                            }
                        },
                    )),
                |domain| domain.get("/shared", grouped_route),
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::post("/shared")
                .header(header::HOST, "a.example.test")
                .body(Body::from("payload"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let response = router
        .oneshot(
            Request::get("/shared")
                .header(header::HOST, "a.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn scoped_domain_fallback_waits_for_normal_routes_and_only_owns_its_host_and_path()
-> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/api/known", grouped_route)
                .fallback(root_fallback_marker)
                .group(
                    RouteGroup::new().domain("a.example.test").prefix("/api"),
                    |domain| domain.fallback(domain_fallback_marker),
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for (host, path, expected) in [
        ("a.example.test", "/api/known", "grouped"),
        ("a.example.test", "/api/missing", "domain fallback"),
        ("a.example.test", "/outside", "root fallback"),
        ("b.example.test", "/api/missing", "root fallback"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{host}{path}");
        assert_eq!(
            to_bytes(response.into_body(), 4096).await?,
            expected,
            "{host}{path}"
        );
    }
    let method = router
        .oneshot(
            Request::post("/api/known")
                .header(header::HOST, "a.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
    Ok(())
}

#[tokio::test]
async fn domain_authority_and_named_url_are_checked() -> TestResult {
    let router = domain_test_router().await?;
    for host in [
        "bad.example.test, evil.test",
        "acme.example.test:99999",
        "acme.example.test:",
        "acme.example.test:abc",
        "bad_name.example.test",
        "user@acme.example.test",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get("/orders/1")
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{host}");
    }
    let response = router
        .clone()
        .oneshot(
            Request::get("/orders/1")
                .header(header::HOST, "acme.example.test")
                .header(header::HOST, "other.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = router
        .clone()
        .oneshot(
            Request::get("http://[::1]/unknown")
                .header(header::HOST, "[0:0:0:0:0:0:0:1]")
                .body(Body::empty())?,
        )
        .await?;
    assert_ne!(response.status(), StatusCode::BAD_REQUEST);
    let response = router
        .clone()
        .oneshot(
            Request::get("http://acme.example.test/orders/1")
                .header(header::HOST, "other.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = router
        .oneshot(
            Request::get("/named-domain")
                .header(header::HOST, "attacker.example.test")
                .header("x-forwarded-host", "evil.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        "https://acme.example.test:8443/base/orders/a%20b%2Fc?page=2"
    );
    let invalid = domain_test_router()
        .await?
        .oneshot(
            Request::get("/named-domain-invalid")
                .header(header::HOST, "attacker.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(invalid.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(invalid).await["code"],
        "http_route_parameter_invalid"
    );
    Ok(())
}

#[tokio::test]
async fn interleaved_same_pattern_routes_do_not_jump_ahead_of_overlapping_domain() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new().domain("{account}.example.test"),
                    |first| first.get("/before", first_domain_marker),
                )
                .group(RouteGroup::new().domain("foo.{region}.test"), |middle| {
                    middle.get("/overlap", second_domain_marker)
                })
                .group(RouteGroup::new().domain("{account}.example.test"), |last| {
                    last.get("/overlap", first_domain_marker)
                })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/overlap")
                .header(header::HOST, "foo.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "second domain");
    let response = router
        .oneshot(
            Request::get("/overlap")
                .header(header::HOST, "bar.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "first domain");
    Ok(())
}

#[tokio::test]
async fn nested_interleaved_domain_routes_keep_each_declaration_position() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new().domain("{account}.example.test"),
                |outer| {
                    outer
                        .get("/before", first_domain_marker)
                        .group(RouteGroup::new().domain("foo.{region}.test"), |middle| {
                            middle.get("/overlap", second_domain_marker)
                        })
                        .get("/overlap", first_domain_marker)
                },
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/overlap")
                .header(header::HOST, "foo.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "second domain");
    let response = router
        .oneshot(
            Request::get("/overlap")
                .header(header::HOST, "bar.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "first domain");
    Ok(())
}

#[tokio::test]
async fn disjoint_same_pattern_buckets_use_last_duplicate_method_route() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new().domain("{account}.example.test"),
                    |first| first.get("/duplicate", first_domain_marker),
                )
                .group(RouteGroup::new().domain("foo.{region}.test"), |middle| {
                    middle.get("/other", second_domain_marker)
                })
                .group(RouteGroup::new().domain("{account}.example.test"), |last| {
                    last.get("/duplicate", second_domain_marker)
                })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::get("/duplicate")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "second domain");
    Ok(())
}

#[tokio::test]
async fn independent_route_tables_keep_merge_declaration_order() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            let first = Routes::new().get("/unrelated", grouped_route).group(
                RouteGroup::new().domain("{account}.example.test"),
                |domain| domain.get("/overlap", first_domain_marker),
            );
            let second = Routes::new()
                .group(RouteGroup::new().domain("foo.{region}.test"), |domain| {
                    domain.get("/overlap", second_domain_marker)
                });
            routes.merge(first).merge(second)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::get("/overlap")
                .header(header::HOST, "foo.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "first domain");
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "同一生产路由表需验证合并域、独立域、fallback、具名 URL 与 middleware"
)]
async fn merged_root_routes_bind_to_current_domain_and_keep_nested_contracts() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.app.url = "https://app.example.test:8443/base".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new()
                    .domain("{account}.example.test")
                    .prefix("/api"),
                |domain| {
                    domain
                        .get("/direct", first_domain_marker)
                        .merge(
                            Routes::new()
                                .get("/merged/{id}", domain_order)
                                .name("merged.show")
                                .layer(axum::middleware::from_fn(
                                    |request: Request, next: Next| async move {
                                        let mut response = next.run(request).await;
                                        response.headers_mut().insert(
                                            "x-merged-middleware",
                                            axum::http::HeaderValue::from_static("applied"),
                                        );
                                        response
                                    },
                                ))
                                .group(
                                    RouteGroup::new().domain("special.example.test"),
                                    |explicit| {
                                        explicit
                                            .get("/explicit", second_domain_marker)
                                            .get("/overlap", second_domain_marker)
                                    },
                                )
                                .get("/overlap", first_domain_marker)
                                .group(
                                    RouteGroup::new().domain("special.example.test"),
                                    |explicit| {
                                        explicit
                                            .get("/named", second_domain_marker)
                                            .name("explicit.named")
                                    },
                                )
                                .fallback(domain_fallback_marker),
                        )
                        .get("/merged-url", merged_named_url)
                        .get("/explicit-url", merged_explicit_named_url)
                },
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for (host, path, expected) in [
        ("acme.example.test", "/api/direct", "first domain"),
        (
            "acme.example.test",
            "/api/merged/42",
            "acme:42:/merged/42:missing:",
        ),
        ("special.example.test", "/api/explicit", "second domain"),
        ("special.example.test", "/api/overlap", "second domain"),
        ("acme.example.test", "/api/missing", "domain fallback"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{host}{path}");
        if path == "/api/merged/42" {
            assert_eq!(response.headers()["x-merged-middleware"], "applied");
        }
        assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
    }
    let generated = router
        .clone()
        .oneshot(
            Request::get("/api/merged-url")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(generated.status(), StatusCode::OK);
    assert_eq!(
        json_body(generated).await["data"],
        "https://acme.example.test:8443/base/api/merged/42"
    );
    let explicit_url = router
        .clone()
        .oneshot(
            Request::get("/api/explicit-url")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(explicit_url.status(), StatusCode::OK);
    assert_eq!(
        json_body(explicit_url).await["data"],
        "https://special.example.test:8443/base/api/named"
    );
    let other_host = router
        .oneshot(
            Request::get("/api/merged/42")
                .header(header::HOST, "unrelated.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(other_host.status(), StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn merged_root_paths_override_same_named_domain_parameter_through_prefix() -> TestResult {
    for (prefix, path) in [("/api", "/{tenant}"), ("/{tenant}", "/x")] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(
                    RouteGroup::new()
                        .domain("{tenant}.example.test")
                        .prefix(prefix),
                    |domain| domain.merge(Routes::new().get(path, route_tenant_collision)),
                )
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        let response = router
            .oneshot(
                Request::get(if prefix == "/api" {
                    "/api/path"
                } else {
                    "/path/x"
                })
                .header(header::HOST, "host.example.test")
                .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["data"]["tenant"], "path");
        assert_eq!(
            json["data"]["parameters"],
            serde_json::json!([["tenant", "path"]])
        );
    }
    Ok(())
}

#[tokio::test]
async fn overlapping_domain_fallbacks_follow_declaration_order_through_merge_and_groups()
-> TestResult {
    for (merged, broad_first) in [(true, true), (true, false), (false, true), (false, false)] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(
                    RouteGroup::new()
                        .domain("{account}.example.test")
                        .prefix("/api"),
                    |domain| {
                        let explicit = |routes: Routes| {
                            routes.group(
                                RouteGroup::new()
                                    .domain("foo.{region}.test")
                                    .prefix("/nested"),
                                |nested| nested.fallback(second_domain_marker),
                            )
                        };
                        match (merged, broad_first) {
                            (true, true) => {
                                domain.merge(explicit(Routes::new().fallback(first_domain_marker)))
                            }
                            (true, false) => {
                                domain.merge(explicit(Routes::new()).fallback(first_domain_marker))
                            }
                            (false, true) => explicit(domain.fallback(first_domain_marker)),
                            (false, false) => explicit(domain).fallback(first_domain_marker),
                        }
                    },
                )
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        let overlap = router
            .clone()
            .oneshot(
                Request::get("/api/nested/missing")
                    .header(header::HOST, "foo.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(overlap.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(overlap.into_body(), 4096).await?,
            if broad_first {
                "first domain"
            } else {
                "second domain"
            },
            "merged={merged}, broad_first={broad_first}"
        );
        let broad_only = router
            .clone()
            .oneshot(
                Request::get("/api/nested/missing")
                    .header(header::HOST, "bar.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(broad_only.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(broad_only.into_body(), 4096).await?,
            "first domain"
        );
        let explicit_only = router
            .clone()
            .oneshot(
                Request::get("/api/nested/missing")
                    .header(header::HOST, "foo.other.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(explicit_only.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(explicit_only.into_body(), 4096).await?,
            "second domain"
        );
    }
    Ok(())
}

#[tokio::test]
async fn same_domain_fallbacks_choose_deepest_scope_in_either_registration_order() -> TestResult {
    for root_first in [true, false] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(RouteGroup::new().domain("a.example.test"), |domain| {
                    let nested = |routes: Routes| {
                        routes.group(RouteGroup::new().prefix("/api"), |api| {
                            api.fallback(first_domain_marker)
                                .group(RouteGroup::new().prefix("/v2"), |v2| {
                                    v2.fallback(second_domain_marker)
                                })
                        })
                    };
                    if root_first {
                        nested(domain.fallback(root_fallback_marker))
                    } else {
                        nested(domain).fallback(root_fallback_marker)
                    }
                })
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for (path, expected) in [
            ("/api/v2/missing", "second domain"),
            ("/api/missing", "first domain"),
            ("/outside", "root fallback"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::get(path)
                        .header(header::HOST, "a.example.test")
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                to_bytes(response.into_body(), 4096).await?,
                expected,
                "root_first={root_first}, path={path}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn unrestricted_fallbacks_choose_deepest_scope_in_either_registration_order() -> TestResult {
    for root_first in [true, false] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                let nested = |routes: Routes| {
                    routes.group(RouteGroup::new().prefix("/api"), |api| {
                        api.fallback(first_domain_marker)
                            .group(RouteGroup::new().prefix("/v2"), |v2| {
                                v2.fallback(second_domain_marker)
                            })
                    })
                };
                if root_first {
                    nested(routes.fallback(root_fallback_marker))
                } else {
                    nested(routes).fallback(root_fallback_marker)
                }
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for (path, expected) in [
            ("/api/v2/missing", "second domain"),
            ("/api/missing", "first domain"),
            ("/outside", "root fallback"),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(path).body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                to_bytes(response.into_body(), 4096).await?,
                expected,
                "root_first={root_first}, path={path}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn enclosing_parameter_or_wildcard_prefix_rejects_explicit_domain_fallback() -> TestResult {
    for (outer, inner) in [
        ("/{id}", ""),
        ("/*rest", ""),
        ("/outer", "/{id}"),
        ("/outer", "/*rest"),
    ] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(RouteGroup::new().prefix(outer), |enclosing| {
                    enclosing.group(RouteGroup::new().prefix(inner), |middle| {
                        middle.group(RouteGroup::new().domain("a.example.test"), |domain| {
                            domain.fallback(domain_fallback_marker)
                        })
                    })
                })
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let error = build_router(&app)
            .err()
            .ok_or("parameterized explicit-domain fallback unexpectedly built")?;
        assert_eq!(
            error.code(),
            "http_fallback_scope_unsupported",
            "outer={outer}, inner={inner}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn unnamed_domain_routes_prefer_path_parameter_over_same_named_domain() -> TestResult {
    for prefix in ["", "/{tenant}"] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(
                    RouteGroup::new()
                        .domain("{tenant}.example.test")
                        .prefix(prefix),
                    |domain| {
                        domain.get(
                            if prefix.is_empty() { "/{tenant}" } else { "/x" },
                            route_tenant_collision,
                        )
                    },
                )
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        let response = router
            .oneshot(
                Request::get(if prefix.is_empty() {
                    "/path"
                } else {
                    "/path/x"
                })
                .header(header::HOST, "host.example.test")
                .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let json = json_body(response).await;
        assert_eq!(json["data"]["tenant"], "path");
        assert_eq!(
            json["data"]["parameters"],
            serde_json::json!([["tenant", "path"]])
        );
    }
    Ok(())
}

#[tokio::test]
async fn absolute_uri_authority_survives_site_root_forward_to_domain_route() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.root_target = "forward:/forwarded".to_owned();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider).provider(TestRoutes::new(|routes| {
        routes.group(RouteGroup::new().domain("{account}.example.test"), |domain| {
            domain.get("/forwarded", |DomainParams(tenant): DomainParams<DomainTenant>, request: Request| async move {
                format!("{}:{}", tenant.account, request.uri())
            })
        })
    }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(Request::get("http://acme.example.test/?q=1").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await?,
        "acme:http://acme.example.test/forwarded?q=1"
    );
    Ok(())
}

#[tokio::test]
async fn domain_registration_rejects_invalid_and_duplicate_parameter_contracts() -> TestResult {
    for (pattern, path, named, expected) in [
        (
            "pre-{account}.example.test",
            "/x",
            false,
            "http_domain_pattern_invalid",
        ),
        (
            "{id}.example.test",
            "/x/{id}",
            true,
            "http_route_parameter_duplicate",
        ),
    ] {
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes.group(RouteGroup::new().domain(pattern), |domain| {
                    let domain = domain.get(path, grouped_route);
                    if named {
                        domain.name("duplicate")
                    } else {
                        domain
                    }
                })
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let error = build_router(&app)
            .err()
            .ok_or("invalid domain route unexpectedly built")?;
        assert_eq!(error.code(), expected);
    }
    Ok(())
}

#[tokio::test]
async fn overlapping_domain_patterns_follow_registration_order_and_nested_groups_override()
-> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new().domain("{account}.example.test"),
                    |outer| {
                        outer
                            .get("/overlap", first_domain_marker)
                            .group(RouteGroup::new().prefix("/nested"), |inherited| {
                                inherited.get("/inherited", first_domain_marker)
                            })
                            .group(
                                RouteGroup::new().domain("override.example.test"),
                                |override_domain| {
                                    override_domain.get("/target", second_domain_marker)
                                },
                            )
                    },
                )
                .group(RouteGroup::new().domain("foo.{region}.test"), |second| {
                    second.get("/overlap", second_domain_marker)
                })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for (host, path, expected) in [
        ("foo.example.test", "/overlap", "first domain"),
        ("a.example.test", "/nested/inherited", "first domain"),
        ("override.example.test", "/target", "second domain"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{host}{path}");
        assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
    }
    let override_wrong_host = router
        .oneshot(
            Request::get("/target")
                .header(header::HOST, "a.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(json_body(override_wrong_host).await["code"], "not_found");
    Ok(())
}

#[tokio::test]
async fn domain_and_root_dispatch_work_over_real_tcp() -> TestResult {
    let router = domain_test_router().await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let result = async {
        let url = format!("http://{address}/orders/9?tcp=1");
        let get = client
            .get(&url)
            .header(header::HOST, "ACME.example.test")
            .send()
            .await?;
        assert_eq!(get.status(), StatusCode::OK);
        assert_eq!(get.text().await?, "acme:9:/orders/9?tcp=1:missing:");
        let post = client
            .post(&url)
            .header(header::HOST, "acme.example.test")
            .body("tcp body")
            .send()
            .await?;
        assert_eq!(post.status(), StatusCode::OK);
        assert_eq!(post.text().await?, "root:/orders/9?tcp=1:tcp body");
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    server.abort();
    let _ = server.await;
    result
}

#[tokio::test]
async fn domain_root_route_takes_priority_over_default_site_root_target() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().domain("a.example.test"), |domain| {
                domain.get("/", first_domain_marker)
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/")
                .header(header::HOST, "a.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "first domain");
    let site = router
        .oneshot(
            Request::get("/")
                .header(header::HOST, "b.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(site.status(), StatusCode::OK);
    assert!(
        to_bytes(site.into_body(), 4096)
            .await?
            .starts_with(b"<!doctype html>")
    );
    Ok(())
}
static REQUEST_CANCELLED: AtomicBool = AtomicBool::new(false);

struct ScopeProbe(Arc<Mutex<Option<Arc<Scope>>>>);

async fn streamed_scoped_identity(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&scope));
    Body::from_stream(
        futures_util::stream::once(async move {
            let identity = scope.resolve::<RequestIdentity>().await;
            Ok::<_, std::io::Error>(bytes::Bytes::from(match identity {
                Ok(identity) => identity.0.to_string(),
                Err(error) => error.code().to_owned(),
            }))
        })
        .chain(futures_util::stream::iter([Ok(bytes::Bytes::from_static(
            b"next",
        ))])),
    )
    .into_response()
}

#[tokio::test]
async fn scoped_factory_survives_response_into_body_until_stream_finishes() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(ScopeProbe(Arc::clone(
            &probe,
        )))))
        .provider(IdentityProvider::new(|_| {
            Box::pin(async { Ok(Arc::new(RequestIdentity(7))) })
        }))
        .provider(TestRoutes::new(|routes| {
            routes.get("/streamed-scope", streamed_scoped_identity)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(
            Request::builder()
                .uri("/streamed-scope")
                .body(Body::empty())?,
        )
        .await?;
    let mut body = response.into_body();
    let scope = probe
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or("missing scope")?;
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    let first = futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
        .await
        .ok_or("missing first frame")??;
    assert_eq!(
        first.into_data().map_err(|_| "expected data")?.as_ref(),
        b"7"
    );
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    let second = futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
        .await
        .ok_or("missing second frame")??;
    assert_eq!(
        second.into_data().map_err(|_| "expected data")?.as_ref(),
        b"next"
    );
    assert!(
        futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
            .await
            .is_none()
    );
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

async fn scoped_empty(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::empty())
}

async fn scoped_error(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::from_stream(futures_util::stream::once(async {
        Err::<bytes::Bytes, _>(std::io::Error::other("stream failed"))
    })))
}

async fn scoped_pending_body(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<ScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    Response::new(Body::from_stream(futures_util::stream::pending::<
        std::result::Result<bytes::Bytes, std::io::Error>,
    >()))
}

struct CancelScopeProbe {
    scope: Arc<Mutex<Option<Arc<Scope>>>>,
    ready: Arc<tokio::sync::Notify>,
}

async fn scoped_pending_handler(
    Extension(scope): Extension<Arc<Scope>>,
    Inject(probe): Inject<CancelScopeProbe>,
    Inject(_identity): Inject<RequestIdentity>,
) -> Response {
    *probe
        .scope
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(scope);
    probe.ready.notify_one();
    std::future::pending::<Response>().await
}

async fn scope_test_router(
    probe: Arc<Mutex<Option<Arc<Scope>>>>,
    ready: Arc<tokio::sync::Notify>,
) -> openrust::Result<axum::Router> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(ScopeProbe(Arc::clone(
            &probe,
        )))))
        .provider(TestInstance(std::sync::Arc::new(CancelScopeProbe {
            scope: probe,
            ready,
        })))
        .provider(IdentityProvider::new(|_| {
            Box::pin(async { Ok(Arc::new(RequestIdentity(7))) })
        }))
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/empty", scoped_empty)
                .get("/error", scoped_error)
                .get("/pending-body", scoped_pending_body)
                .get("/pending-handler", scoped_pending_handler)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    build_router(&app)
}

fn observed_scope(probe: &Mutex<Option<Arc<Scope>>>) -> TestResultScope {
    probe
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| "missing scope".into())
}

type TestResultScope = Result<Arc<Scope>, Box<dyn std::error::Error>>;

#[tokio::test]
async fn scope_closes_when_response_body_dropped_or_empty() -> TestResult {
    for path in ["/pending-body", "/empty"] {
        let probe = Arc::new(Mutex::new(None));
        let router =
            scope_test_router(Arc::clone(&probe), Arc::new(tokio::sync::Notify::new())).await?;
        let response = router
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        let body = response.into_body();
        let scope = observed_scope(&probe)?;
        assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
        drop(body);
        assert_eq!(
            scope
                .resolve::<RequestIdentity>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );
    }
    Ok(())
}

#[tokio::test]
async fn scope_closes_on_body_error_before_body_drop() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let router =
        scope_test_router(Arc::clone(&probe), Arc::new(tokio::sync::Notify::new())).await?;
    let response = router
        .oneshot(Request::builder().uri("/error").body(Body::empty())?)
        .await?;
    let mut body = response.into_body();
    let scope = observed_scope(&probe)?;
    assert!(
        futures_util::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
            .await
            .ok_or("missing error frame")?
            .is_err()
    );
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn cancelling_handler_closes_scope_even_if_scope_is_retained() -> TestResult {
    let probe = Arc::new(Mutex::new(None));
    let ready = Arc::new(tokio::sync::Notify::new());
    let router = scope_test_router(Arc::clone(&probe), Arc::clone(&ready)).await?;
    let request = Request::builder()
        .uri("/pending-handler")
        .body(Body::empty())?;
    let request = tokio::spawn(async move { router.oneshot(request).await });
    ready.notified().await;
    let scope = observed_scope(&probe)?;
    assert_eq!(scope.resolve::<RequestIdentity>().await?.0, 7);
    request.abort();
    assert!(request.await.is_err());
    assert_eq!(
        scope
            .resolve::<RequestIdentity>()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("scope_closed".to_owned())
    );
    Ok(())
}

#[derive(Debug)]
struct RequestIdentity(usize);

async fn scoped_identity(
    Inject(first): Inject<RequestIdentity>,
    Inject(second): Inject<RequestIdentity>,
) -> String {
    format!("{}:{}", first.0, second.0)
}

#[tokio::test]
async fn inject_resolves_async_scoped_factory_once_per_request() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider);
    builder.provider(IdentityProvider::new({
        let calls = Arc::clone(&calls);
        move |_| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                tokio::task::yield_now().await;
                Ok(Arc::new(RequestIdentity(
                    calls.fetch_add(1, Ordering::SeqCst) + 1,
                )))
            })
        }
    }));
    builder.provider(TestRoutes::new(|routes| {
        routes.get("/scoped-identity", scoped_identity)
    }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for expected in ["1:1", "2:2"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/scoped-identity")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await?;
        assert_eq!(body.as_ref(), expected.as_bytes());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

fn is_utc_rfc3339(value: &serde_json::Value) -> bool {
    let Some(value) = value.as_str() else {
        return false;
    };
    let bytes = value.as_bytes();
    value.ends_with('Z')
        && bytes.get(19) == Some(&b'.')
        && bytes
            .get(20..23)
            .is_some_and(|millis| millis.iter().all(u8::is_ascii_digit))
        && chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

struct Greeting(&'static str);

#[derive(Deserialize)]
struct CreatePost {
    title: String,
    pages: u32,
}

impl Validate for CreatePost {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "title",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::custom(|field| {
                        if field
                            .value
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|value| value.chars().count() < 3)
                        {
                            return Err(validator::ValidationError::new("length").with_message(
                                std::borrow::Cow::Borrowed("title must be at least 3 characters"),
                            ));
                        }
                        Ok(())
                    }),
                ],
            )
            .field(
                "pages",
                [
                    Rule::required(),
                    Rule::integer(),
                    Rule::range(Some(1), None),
                ],
            )
    }
}

async fn greet(Inject(greeting): Inject<Greeting>) -> Json<&'static str> {
    Json(greeting.0)
}

async fn create(Validated(Json(input)): Validated<Json<CreatePost>>) -> Json<String> {
    let _ = input.pages;
    Json(input.title)
}

async fn missing(Inject(_greeting): Inject<String>) -> &'static str {
    "unreachable"
}

async fn global_layer_value(Extension(value): Extension<&'static str>) -> &'static str {
    value
}

async fn add_shared_extension(mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    request.extensions_mut().insert("shared");
    let response = next.run(request).await;
    if path == "/plain-error" {
        (StatusCode::INTERNAL_SERVER_ERROR, "private detail").into_response()
    } else {
        response
    }
}

// 此处的 panic 是被测行为：它必须转换为 500 envelope，不能导致服务器退出。
#[allow(clippy::panic)]
async fn boom() -> &'static str {
    panic!("handler exploded")
}

async fn router() -> openrust::Result<axum::Router> {
    router_with_config(FrameworkConfig::default()).await
}

async fn router_with_config(config: FrameworkConfig) -> openrust::Result<axum::Router> {
    let mut config = config;
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    router_with_exact_config(config).await
}

async fn router_with_exact_config(config: FrameworkConfig) -> openrust::Result<axum::Router> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(Greeting("hello"))))
        .provider(TestRoutes::new(|r| {
            r.nest("/api", |api| {
                api.get("/greet", greet)
                    .post("/posts", create)
                    .get("/missing", missing)
                    .get("/boom", boom)
                    .get("/large", large)
                    .get("/slow", slow)
                    .get("/execution", execution_context_info)
                    .get("/cancel", cancellation_probe)
                    .post("/raw", create_raw)
                    .get("/items/{id}", item)
                    .get("/search", search)
                    .get("/own-404", own_not_found)
                    .post("/created", created)
                    .delete("/no-content", no_content)
                    .get("/page", page)
                    .get("/limited", limited)
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    build_router(&app)
}

#[derive(Deserialize, serde::Serialize)]
struct RawPost {
    title: String,
    pages: u32,
}

async fn create_raw(Json(input): Json<RawPost>) -> Json<RawPost> {
    Json(input)
}
async fn item(Path(id): Path<u64>) -> Json<u64> {
    Json(id)
}
async fn search(Query(id): Query<RawPost>) -> Json<String> {
    Json(id.title)
}
async fn own_not_found() -> openrust::Error {
    openrust::Error::NotFound("my resource is absent".into())
}

async fn grouped_route() -> &'static str {
    "grouped"
}

async fn generated_named_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    let parameters = std::collections::BTreeMap::from([
        ("id".to_owned(), "a b/c".to_owned()),
        ("page".to_owned(), "2".to_owned()),
        ("filter".to_owned(), "tea & cake".to_owned()),
    ]);
    urls.route("admin.users.show", &parameters).map(Json)
}

async fn generated_named_url_missing_parameter(
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<String>> {
    urls.route("admin.users.show", &std::collections::BTreeMap::new())
        .map(Json)
}

async fn generated_outside_group_name(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    urls.route(
        "admin.management.outside",
        &std::collections::BTreeMap::new(),
    )
    .map(Json)
}

async fn generated_trailing_slash_url(urls: RouteUrlGenerator) -> openrust::Result<Json<String>> {
    urls.route("posts.trailing", &std::collections::BTreeMap::new())
        .map(Json)
}

async fn grouped_order(Extension(order): Extension<Vec<&'static str>>) -> Json<Vec<&'static str>> {
    Json(order)
}

async fn grouped_outside() -> Json<Vec<&'static str>> {
    Json(Vec::new())
}

async fn mark_route_group(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-route-group", axum::http::HeaderValue::from_static("api"));
    response
}

async fn mark_first_group_middleware(mut request: Request, next: Next) -> Response {
    request.extensions_mut().insert(true);
    next.run(request).await
}

async fn observe_first_group_middleware(request: Request, next: Next) -> Response {
    let first_middleware_ran = request.extensions().get::<bool>().copied().unwrap_or(false);
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-middleware-order",
        axum::http::HeaderValue::from_static(if first_middleware_ran {
            "declared"
        } else {
            "reversed"
        }),
    );
    response
}

async fn record_group_middleware(
    axum::extract::State(label): axum::extract::State<&'static str>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(order) = request.extensions_mut().get_mut::<Vec<&'static str>>() {
        order.push(label);
    } else {
        request.extensions_mut().insert(vec![label]);
    }
    next.run(request).await
}

#[tokio::test]
async fn nested_route_groups_combine_prefixes_and_scope_middleware() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new()
                        .prefix("api/")
                        .middleware(axum::middleware::from_fn(mark_route_group))
                        .middleware(axum::middleware::from_fn(mark_first_group_middleware))
                        .middleware(axum::middleware::from_fn(observe_first_group_middleware))
                        .middleware(axum::middleware::from_fn_with_state(
                            "outer-first",
                            record_group_middleware,
                        ))
                        .middleware(axum::middleware::from_fn_with_state(
                            "outer-second",
                            record_group_middleware,
                        )),
                    |api| {
                        api.group(
                            RouteGroup::new().prefix("/v1/").middleware(
                                axum::middleware::from_fn_with_state(
                                    "inner",
                                    record_group_middleware,
                                ),
                            ),
                            |v1| v1.get("/grouped", grouped_order),
                        )
                    },
                )
                .get("/outside-group", grouped_route)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;

    let response = build_router(&app)?
        .oneshot(
            Request::builder()
                .uri("/api/v1/grouped")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-route-group"], "api");
    assert_eq!(response.headers()["x-middleware-order"], "declared");
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["outer-first", "outer-second", "inner"])
    );

    let response = build_router(&app)?
        .oneshot(
            Request::builder()
                .uri("/outside-group")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("x-route-group"));
    Ok(())
}

struct NamedMiddlewareProvider;

impl openrust::core::ServiceProvider for NamedMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias(
                "first",
                axum::middleware::from_fn_with_state("first", record_group_middleware),
            )?;
            app.middleware_alias(
                "second",
                axum::middleware::from_fn_with_state("second", record_group_middleware),
            )?;
            app.middleware_group("composed", ["first", "second"])?;
            app.middleware_group("nested", ["composed", "first"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn named_middleware_aliases_and_nested_groups_keep_declaration_order() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(NamedMiddlewareProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/route-scope/private", grouped_order)
                .middleware_alias("second")
                .get("/route-scope/public", grouped_outside)
                .group(
                    RouteGroup::new()
                        .prefix("/private")
                        .middleware_group("nested"),
                    |private| private.get("/named", grouped_order),
                )
                .group(
                    RouteGroup::new()
                        .prefix("/alias")
                        .middleware_alias("second"),
                    |private| private.get("/named", grouped_order),
                )
                .get("/outside", grouped_outside)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/private/named")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["first", "second"])
    );

    let response = router
        .clone()
        .oneshot(Request::builder().uri("/alias/named").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["second"])
    );

    let response = router
        .clone()
        .oneshot(Request::builder().uri("/outside").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], serde_json::json!([]));

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/route-scope/private")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["second"])
    );

    let response = router
        .oneshot(
            Request::builder()
                .uri("/route-scope/public")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], serde_json::json!([]));
    Ok(())
}

struct DuplicateMiddlewareProvider;

struct PriorityMiddlewareProvider;

impl openrust::core::ServiceProvider for PriorityMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias(
                "first",
                axum::middleware::from_fn_with_state("first", record_group_middleware),
            )?;
            app.middleware_alias(
                "second",
                axum::middleware::from_fn_with_state("second", record_group_middleware),
            )?;
            app.middleware_priority(["second"])?;
            app.prepend_middleware_priority("first")?;
            app.append_middleware_priority("third")?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn provider_priority_sorts_named_route_layers_after_group_flattening() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(PriorityMiddlewareProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().middleware_alias("second"), |outer| {
                outer.group(
                    RouteGroup::new()
                        .middleware(axum::middleware::from_fn_with_state(
                            "anonymous",
                            record_group_middleware,
                        ))
                        .middleware_alias("first"),
                    |inner| inner.get("/priority", grouped_order),
                )
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/priority").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        serde_json::json!(["first", "second", "anonymous"])
    );
    Ok(())
}

struct RejectingAuthProvider;

async fn reject_as_unauthorized(_request: Request, _next: Next) -> Response {
    StatusCode::UNAUTHORIZED.into_response()
}

impl openrust::core::ServiceProvider for RejectingAuthProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias("auth", axum::middleware::from_fn(reject_as_unauthorized))?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn default_precognitive_priority_runs_before_route_authorization() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(RejectingAuthProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new()
                    .middleware_alias("auth")
                    .middleware_alias("precognitive"),
                |routes| routes.get("/auth-before-handler", || async { "unreachable" }),
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(
            Request::get("/auth-before-handler")
                .header("precognition", "true")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["precognition"], "true");
    assert!(
        response.headers()[header::VARY]
            .to_str()?
            .contains("Precognition")
    );
    assert!(!response.headers().contains_key("precognition-success"));
    Ok(())
}

impl openrust::core::ServiceProvider for DuplicateMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_alias("shared", axum::middleware::from_fn(mark_route_group))?;
            app.middleware_group("shared", ["shared"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn duplicate_middleware_names_fail_provider_boot() -> TestResult {
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(DuplicateMiddlewareProvider);
    let error = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(FrameworkConfig::default()),
        ))
        .await
        .err()
        .ok_or("duplicate middleware name unexpectedly started")?;
    assert_eq!(error.code(), "http_middleware_name_duplicate");
    Ok(())
}

#[tokio::test]
async fn unknown_or_cyclic_named_middleware_fails_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().middleware_alias("missing"), |group| {
                group.get("/unknown-middleware", grouped_order)
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("unknown named middleware unexpectedly built")?;
    assert_eq!(error.code(), "http_middleware_name_unknown");

    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(CyclicMiddlewareProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().middleware_group("first"), |group| {
                group.get("/cyclic-middleware", grouped_order)
            })
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(FrameworkConfig::default()),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("cyclic named middleware unexpectedly built")?;
    assert_eq!(error.code(), "http_middleware_group_cycle");
    Ok(())
}

#[test]
fn named_middleware_without_registry_fails_closed_on_all_route_paths() -> TestResult {
    let direct = openrust::http::Routes::new()
        .get("/private", grouped_order)
        .middleware_alias("auth")
        .get("/public", grouped_outside)
        .into_router()
        .err()
        .ok_or("named middleware without an application registry was silently ignored")?;
    assert_eq!(direct.code(), "http_middleware_registry_unavailable");

    let nested = openrust::http::Routes::new()
        .group(RouteGroup::new(), |routes| {
            routes
                .get("/private", grouped_order)
                .middleware_group("auth")
        })
        .into_router()
        .err()
        .ok_or("nested named middleware without a registry was silently ignored")?;
    assert_eq!(nested.code(), "http_middleware_registry_unavailable");

    let group = openrust::http::Routes::new()
        .group(RouteGroup::new().middleware_alias("auth"), |routes| {
            routes.get("/private", grouped_order)
        })
        .into_router()
        .err()
        .ok_or("group middleware without a registry was silently ignored")?;
    assert_eq!(group.code(), "http_middleware_registry_unavailable");
    Ok(())
}

struct CyclicMiddlewareProvider;

impl openrust::core::ServiceProvider for CyclicMiddlewareProvider {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.middleware_group("first", ["second"])?;
            app.middleware_group("second", ["first"])?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn duplicate_routes_across_groups_dispatch_to_last_registration() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(RouteGroup::new().prefix("api"), |api| {
                    api.get("/duplicate", first_domain_marker)
                })
                .group(RouteGroup::new().prefix("/api/"), |api| {
                    api.get("/duplicate", second_domain_marker)
                })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .oneshot(Request::get("/api/duplicate").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "second domain");
    Ok(())
}

async fn secure_redirect_response(info: openrust::http::RequestInfo) -> openrust::Result<Response> {
    Ok(openrust::http::Redirector::from_info(&info)
        .secure("/login?source=protected")?
        .into_response())
}

#[cfg(feature = "session")]
async fn guest_redirect_response(info: openrust::http::RequestInfo) -> openrust::Result<Response> {
    // 通过受管理Router的匿名Route验证，而非测试手工声明的命中事实。
    assert!(info.route()?.is_some());
    assert_eq!(info.route_name()?, None);
    Ok(openrust::http::Redirector::from_info(&info)
        .guest("/login")?
        .into_response())
}

#[cfg(feature = "session")]
async fn intended_redirect_response(
    info: openrust::http::RequestInfo,
) -> openrust::Result<Response> {
    Ok(openrust::http::Redirector::from_info(&info)
        .intended_or("/fallback")?
        .into_response())
}

async fn refresh_redirect_response(
    info: openrust::http::RequestInfo,
) -> openrust::Result<Response> {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-refresh-proof",
        axum::http::HeaderValue::from_static("kept"),
    );
    Ok(openrust::http::Redirector::from_info(&info)
        .refresh_with(StatusCode::SEE_OTHER, headers)?
        .into_response())
}

async fn away_redirect_response(
    info: openrust::http::RequestInfo,
    Path(mode): Path<String>,
) -> openrust::Result<Response> {
    let redirect = openrust::http::Redirector::from_info(&info);
    let response = match mode.as_str() {
        "empty" => redirect.away("")?,
        "unsafe" => redirect.away("/bad\r\nX-Test: injected")?,
        "bad-status" => {
            redirect.away_with("relative", StatusCode::OK, axum::http::HeaderMap::new())?
        }
        "custom" => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("x-away-proof", axum::http::HeaderValue::from_static("kept"));
            redirect.away_with("https://outside.test/guide", StatusCode::SEE_OTHER, headers)?
        }
        _ => redirect.away("relative?tag=<b>&page=1")?,
    };
    Ok(response.into_response())
}

#[tokio::test]
async fn refresh_and_away_public_routes_keep_distinct_targets_without_session_provider()
-> TestResult {
    // 复用无Session Provider应用，验证真实RequestInfo path和公开错误响应链。
    let router = intent_redirect_router().await?;
    for (path, status, location, proof) in [
        (
            "/refresh/a%2Fb?token=drop",
            StatusCode::SEE_OTHER,
            "http://example.test/refresh/a%2Fb",
            Some("x-refresh-proof"),
        ),
        (
            "/away/relative",
            StatusCode::FOUND,
            "relative?tag=<b>&page=1",
            None,
        ),
        (
            "/away/custom",
            StatusCode::SEE_OTHER,
            "https://outside.test/guide",
            Some("x-away-proof"),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, "example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::LOCATION], location);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        if let Some(proof) = proof {
            assert_eq!(response.headers()[proof], "kept");
        }
        let body = to_bytes(response.into_body(), 16384).await?;
        let html = std::str::from_utf8(&body)?;
        assert!(
            html.contains(
                &location
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            )
        );
        if path.starts_with("/refresh") {
            assert!(!html.contains("token=drop"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn away_public_route_rejects_empty_unsafe_target_and_non_redirect_status() -> TestResult {
    let router = intent_redirect_router().await?;
    for (mode, code) in [
        ("empty", "redirect_location_invalid"),
        ("unsafe", "redirect_location_invalid"),
        ("bad-status", "redirect_status_invalid"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/away/{mode}"))
                    .header(header::HOST, "example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key(header::LOCATION));
        let body = to_bytes(response.into_body(), 16384).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["code"], code);
    }
    Ok(())
}

async fn intent_redirect_router() -> Result<axum::Router, Box<dyn std::error::Error>> {
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            let routes = routes
                .get("/secure", secure_redirect_response)
                .get("/refresh/{tail}", refresh_redirect_response)
                .get("/away/{mode}", away_redirect_response);
            #[cfg(feature = "session")]
            let routes = routes
                .get("/protected", guest_redirect_response)
                .get("/complete", intended_redirect_response);
            routes
        }));
    let app = builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(FrameworkConfig::default()),
        ))
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    Ok(build_router(&app)?)
}

#[tokio::test]
async fn secure_redirect_public_route_works_without_session_provider() -> TestResult {
    let router = intent_redirect_router().await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/secure")
                .header(header::HOST, "example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers()[header::LOCATION],
        "https://example.test/login?source=protected"
    );
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    #[cfg(feature = "session")]
    {
        let response = router
            .oneshot(
                Request::get("/protected")
                    .header(header::HOST, "example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), 16384).await?;
        let json: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(json["code"], "session_layer_missing");
    }
    Ok(())
}

#[cfg(feature = "session")]
#[tokio::test]
async fn guest_anonymous_route_and_intended_use_same_request_session_once() -> TestResult {
    // 此层只证明公开提取/真实匿名Route接线；持久Cookie与多客户端由唯一模块验收。
    let session = openrust::session::Session::new(
        None,
        Arc::new(openrust::session::MemorySessionStore::default()),
        None,
    )?;
    let router = intent_redirect_router()
        .await?
        .layer(Extension(session.clone()));
    let response = router
        .clone()
        .oneshot(
            Request::get("/protected?next=two&first=one")
                .header(header::HOST, "example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers()[header::LOCATION],
        "http://example.test/login"
    );
    assert_eq!(
        session.get_json("url.intended")?,
        Some(serde_json::json!(
            "http://example.test/protected?first=one&next=two"
        ))
    );
    for expected in [
        "http://example.test/protected?first=one&next=two",
        "http://example.test/fallback",
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get("/complete")
                    .header(header::HOST, "example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(response.headers()[header::LOCATION], expected);
        assert_eq!(session.get_json("url.intended")?, None);
    }
    Ok(())
}

async fn named_redirect_response(
    info: openrust::http::RequestInfo,
    Path(mode): Path<String>,
) -> openrust::Result<Response> {
    #[cfg(feature = "session")]
    assert!(
        !info.has_session(),
        "普通命名跳转fixture不能依赖Session Provider"
    );
    let redirect = openrust::http::Redirector::from_info(&info);
    let parameters = std::collections::BTreeMap::from([
        ("tenant".into(), "Acme".into()),
        ("id".into(), "a b/c".into()),
        ("filter".into(), "tea & cake".into()),
        ("page".into(), "2".into()),
    ]);
    let response = match mode.as_str() {
        "default" => redirect.route("catalog.index", &std::collections::BTreeMap::new())?,
        "unknown" => redirect.route("not.registered", &parameters)?,
        "missing" => redirect.route("catalog.show", &std::collections::BTreeMap::new())?,
        "custom" | "invalid-status" => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(
                "x-named-proof",
                "preserved".parse().map_err(openrust::Error::internal)?,
            );
            headers.insert(
                header::CACHE_CONTROL,
                "private, max-age=60"
                    .parse()
                    .map_err(openrust::Error::internal)?,
            );
            headers.insert(
                header::LOCATION,
                "https://ignored.example/"
                    .parse()
                    .map_err(openrust::Error::internal)?,
            );
            redirect.route_with(
                "catalog.show",
                &parameters,
                if mode == "custom" {
                    StatusCode::SEE_OTHER
                } else {
                    StatusCode::OK
                },
                headers,
            )?
        }
        _ => redirect.route("catalog.show", &parameters)?,
    };
    Ok(response.into_response())
}

async fn named_redirect_router() -> Result<axum::Router, Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.app.url = "https://configured.example.test:8443/base".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/jump/{mode}", named_redirect_response)
                .get("/index", grouped_route)
                .name("catalog.index")
                .group(
                    RouteGroup::new()
                        .prefix("/catalog")
                        .name_prefix("catalog.")
                        .domain("{tenant}.example.test"),
                    |catalog| catalog.get("/items/{id}", grouped_route).name("show"),
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    Ok(build_router(&app)?)
}

#[tokio::test]
async fn named_redirects_use_application_generator_and_existing_response_contract() -> TestResult {
    let router = named_redirect_router().await?;
    let domain_target =
        "https://acme.example.test:8443/base/catalog/items/a%20b%2Fc?filter=tea+%26+cake&page=2";
    for (mode, status, target) in [
        (
            "default",
            StatusCode::FOUND,
            "https://configured.example.test:8443/base/index",
        ),
        ("domain", StatusCode::FOUND, domain_target),
        ("custom", StatusCode::SEE_OTHER, domain_target),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/jump/{mode}"))
                    .header(header::HOST, "attacker.example")
                    .header("x-forwarded-host", "another-attacker.example")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::LOCATION], target);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            if mode == "custom" {
                "private, max-age=60"
            } else {
                "no-cache, private"
            }
        );
        if mode == "custom" {
            assert_eq!(response.headers()["x-named-proof"], "preserved");
        }
        let body = to_bytes(response.into_body(), 16384).await?;
        let html = std::str::from_utf8(&body)?;
        assert!(html.contains(&target.replace('&', "&amp;")));
        if mode != "default" {
            assert!(!html.contains("cake&page=2"));
        }
    }
    Ok(())
}

#[tokio::test]
async fn named_redirect_generation_failures_keep_existing_error_codes() -> TestResult {
    let router = named_redirect_router().await?;
    for (mode, status, code) in [
        (
            "unknown",
            StatusCode::INTERNAL_SERVER_ERROR,
            "http_named_route_not_found",
        ),
        (
            "missing",
            StatusCode::INTERNAL_SERVER_ERROR,
            "http_route_parameter_missing",
        ),
        (
            "invalid-status",
            StatusCode::BAD_REQUEST,
            "redirect_status_invalid",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(format!("/jump/{mode}"))
                    .header(header::HOST, "attacker.example")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert!(!response.headers().contains_key(header::LOCATION));
        assert_eq!(json_body(response).await["code"], code);
    }
    Ok(())
}

#[tokio::test]
async fn named_route_urls_use_group_prefix_parameters_and_configured_host() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        serde_json::json!({
            "app": {"url": "https://app.example.test/root"},
            "api": {"http_status": "semantic"}
        }),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .group(
                    RouteGroup::new().prefix("api/").name_prefix("admin."),
                    |api| {
                        api.group(RouteGroup::new().name_prefix("users."), |users| {
                            users.get("/users/{id}", generated_named_url).name("show")
                        })
                        .get("/missing", generated_named_url_missing_parameter)
                    },
                )
                .group(
                    RouteGroup::new().prefix("/root").name_prefix("admin."),
                    |root| {
                        root.group(
                            RouteGroup::new().prefix("/api").name_prefix("management."),
                            |api| api.get("/outside-name", generated_outside_group_name),
                        )
                    },
                )
                .name("outside")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/users/1")
                .header(header::HOST, "attacker.example")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        "https://app.example.test/root/api/users/a%20b%2Fc?filter=tea+%26+cake&page=2"
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/root/api/outside-name")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["data"],
        "https://app.example.test/root/root/api/outside-name"
    );

    let response = router
        .oneshot(Request::builder().uri("/api/missing").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["code"],
        "http_route_parameter_missing"
    );
    Ok(())
}

#[tokio::test]
async fn named_url_preserves_trailing_slash_and_targets_a_real_route() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/generate-trailing", generated_trailing_slash_url)
                .get("/api/posts/", grouped_route)
                .name("posts.trailing")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/generate-trailing")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let generated = json_body(response).await["data"]
        .as_str()
        .ok_or("generated URL was not a string")?
        .to_owned();
    let path = ::url::Url::parse(&generated)?.path().to_owned();
    assert_eq!(path, "/api/posts/");
    let response = router
        .oneshot(Request::builder().uri(path).body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn duplicate_named_routes_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/first", grouped_route)
                .name("shared")
                .get("/second", grouped_route)
                .name("shared")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("duplicate route names unexpectedly built")?;
    assert_eq!(error.code(), "http_route_name_duplicate");
    Ok(())
}

#[tokio::test]
async fn named_wildcard_routes_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/files/{*rest}", grouped_route)
                .name("files.show")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named wildcard route unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

#[tokio::test]
async fn named_routes_with_empty_segments_fail_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().prefix("/api"), |api| {
                api.get("/files//", grouped_route).name("files.latest")
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named route with empty path segment unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

#[tokio::test]
async fn named_group_root_with_repeated_slashes_fails_during_router_build() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(RouteGroup::new().prefix("/api"), |api| {
                api.get("//", grouped_route).name("api.double-root")
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let error = build_router(&app)
        .err()
        .ok_or("named route with repeated root slashes unexpectedly built")?;
    assert_eq!(error.code(), "http_route_pattern_unsupported");
    Ok(())
}

async fn created() -> openrust::Result<openrust::http::Located<Json<u64>>> {
    Created::with_location("/api/items/42", Json(42))
}

async fn no_content() -> NoContent {
    NoContent
}

async fn page() -> Paginated<u64> {
    Paginated::new(vec![1, 2], PageParams::default(), 57)
}

async fn rate_limited_ok() -> &'static str {
    "ok"
}

async fn limited() -> openrust::Error {
    openrust::Error::rate_limited(30)
}

async fn large() -> Json<String> {
    Json("compressible response ".repeat(100))
}

async fn slow() -> Json<&'static str> {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Json("late")
}

async fn execution_context_info() -> Json<serde_json::Value> {
    let current = ExecutionContext::current();
    Json(serde_json::json!({
        "request_id": current.as_ref().and_then(|context| context.metadata().request_id()),
        "has_budget": current.as_ref().and_then(ExecutionContext::remaining_budget).is_some(),
    }))
}

async fn cancellation_probe() -> &'static str {
    if let Some(context) = ExecutionContext::current() {
        tokio::spawn(async move {
            context.cancelled().await;
            REQUEST_CANCELLED.store(true, Ordering::SeqCst);
        });
    }
    std::future::pending::<&'static str>().await
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tokio::test]
async fn health_and_request_id_are_installed_by_the_plugin() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/health").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().contains_key("x-request-id"),
        "request id is propagated"
    );
    let body = json_body(response).await;
    assert_eq!(body["success"], true);
    assert_eq!(body["code"], "ok");
    assert_eq!(body["message"], "ok");
    assert_eq!(body["data"]["status"], "ok");
    assert!(body.get("timestamp").is_none());
    assert!(body.get("request_id").is_none());
    Ok(())
}

#[tokio::test]
async fn app_builder_http_layer_wraps_routes() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(SharedHttpLayer)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/shared", global_layer_value)
                .get("/plain-error", global_layer_value)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/shared").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await?;
    assert_eq!(body, "shared");

    let response = build_router(&app)?
        .oneshot(Request::get("/plain-error").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let json = json_body(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["code"], "internal_error");
    assert_ne!(json["message"], "private detail");
    Ok(())
}

#[derive(Clone, Default)]
struct TestRateStore(Arc<Mutex<std::collections::HashMap<String, u64>>>);

impl RateLimitStore for TestRateStore {
    fn consume<'a>(
        &'a self,
        key: &'a str,
        _window: Duration,
    ) -> RateLimitFuture<'a, openrust::Result<RateLimitBucket>> {
        Box::pin(async move {
            let mut counts = self
                .0
                .lock()
                .map_err(|_| std::io::Error::other("test store poisoned"))?;
            let count = counts.entry(key.to_owned()).or_default();
            *count += 1;
            Ok(RateLimitBucket::new(*count, 23))
        })
    }
}

#[tokio::test]
async fn route_rate_limit_uses_scoped_custom_keys_and_returns_retry_after() -> TestResult {
    let layer = RateLimitLayer::with_key("posts:list", 1, Duration::from_secs(60), |request| {
        request
            .headers()
            .get("x-test-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or(openrust::Error::BadRequest("missing test key".into()))
    });
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            TestRateStore::default(),
        ))))
        .provider(TestRoutes::new(move |routes| {
            routes.get("/limited", rate_limited_ok).layer(layer.layer())
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let first = router
        .clone()
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "a")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(first.status(), StatusCode::OK);
    let limited = router
        .clone()
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "a")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        limited
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("23")
    );
    let isolated = router
        .oneshot(
            Request::get("/limited")
                .header("x-test-key", "b")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(isolated.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn rate_limit_and_handler_share_the_request_scoped_factory() -> TestResult {
    struct ScopedLimiter(Arc<AtomicUsize>);
    impl openrust::core::ServiceProvider for ScopedLimiter {
        fn register<'a>(
            &'a self,
            app: &'a mut openrust::core::RegisterContext<'_>,
        ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
            Box::pin(async move {
                let calls = Arc::clone(&self.0);
                app.scoped::<RateLimiter, _>(move |_| {
                    let calls = Arc::clone(&calls);
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Arc::new(RateLimiter::new(TestRateStore::default())))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }
    async fn handler(Inject(_limiter): Inject<RateLimiter>) -> &'static str {
        "ok"
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(ScopedLimiter(Arc::clone(&calls)))
        .provider(TestRoutes::new(|routes| {
            routes.get("/scoped-limit", handler).layer(
                RateLimitLayer::with_key("scope", 10, Duration::from_secs(60), |_| {
                    Ok("one".to_owned())
                })
                .layer(),
            )
        }));
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/scoped-limit").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn authenticated_rate_limit_rejects_a_missing_identity() -> TestResult {
    let layer = RateLimitLayer::by_user("profile:read", 10, Duration::from_secs(60));
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
        .merge(figment::Figment::new());
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestInstance(std::sync::Arc::new(RateLimiter::new(
            TestRateStore::default(),
        ))))
        .provider(TestRoutes::new(move |routes| {
            routes
                .get("/limited-user", rate_limited_ok)
                .layer(layer.layer())
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("/limited-user").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn inject_resolves_registered_services() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/greet").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "hello");
    Ok(())
}

#[tokio::test]
async fn inject_of_unregistered_service_is_a_envelope_not_a_panic() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/missing").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json_body(response).await;
    assert_eq!(body["code"], "service_not_bound");
    assert_eq!(body["message"], "internal server error");
    Ok(())
}

#[tokio::test]
async fn validated_json_reports_each_field() -> TestResult {
    let request = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"ab","pages":0}"#))?;
    let response = router().await?.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert_eq!(
        body["errors"]["title"][0],
        "title must be at least 3 characters"
    );
    assert_eq!(body["errors"]["pages"][0], "range");
    Ok(())
}

#[tokio::test]
async fn validated_json_accepts_valid_input_and_rejects_malformed_bodies() -> TestResult {
    let ok = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"Hello","pages":3}"#))?;
    let response = router().await?.oneshot(ok).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "Hello");

    let malformed = Request::post("/api/posts")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))?;
    let response = router().await?.oneshot(malformed).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "bad_request");
    Ok(())
}

#[tokio::test]
async fn panics_become_500_envelopes() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/boom").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["code"], "internal_error");
    Ok(())
}

#[tokio::test]
async fn compression_negotiates_gzip_and_br_and_respects_absence() -> TestResult {
    for (accept, expected) in [
        (Some("gzip"), Some("gzip")),
        (Some("br"), Some("br")),
        (None, None),
    ] {
        let mut request = Request::get("/api/large");
        if let Some(accept) = accept {
            request = request.header(header::ACCEPT_ENCODING, accept);
        }
        let response = router()
            .await?
            .oneshot(request.body(Body::empty())?)
            .await?;
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            expected
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        if expected.is_some() {
            assert!(bytes.len() < 2200);
        } else {
            let expected_body = serde_json::json!({
                "success": true,
                "code": "ok",
                "message": "ok",
                "data": "compressible response ".repeat(100),
            });
            assert_eq!(bytes.len(), serde_json::to_vec(&expected_body)?.len());
        }
    }
    Ok(())
}

#[tokio::test]
async fn public_site_serves_index_and_maps_files_from_configured_directory() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("index.html"), "site home")?;
    std::fs::write(directory.path().join("app.js"), "site asset")?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    let app = router_with_exact_config(config).await?;

    let home = app
        .clone()
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(home.status(), StatusCode::OK);
    assert_eq!(home.headers()[header::CONTENT_TYPE], "text/html");
    assert_eq!(to_bytes(home.into_body(), usize::MAX).await?, "site home");

    let asset = app
        .oneshot(Request::get("/app.js").body(Body::empty())?)
        .await?;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(to_bytes(asset.into_body(), usize::MAX).await?, "site asset");
    Ok(())
}

async fn fallback_echo(request: Request) -> String {
    let uri = request.uri().to_string();
    let marker = request
        .headers()
        .get("x-fallback-marker")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing")
        .to_owned();
    let body = to_bytes(request.into_body(), 1024).await.map_or_else(
        |_| "unreadable".to_owned(),
        |body| String::from_utf8_lossy(&body).into_owned(),
    );
    format!("{uri}|{marker}|{body}")
}

#[tokio::test]
async fn fallback_runs_only_for_unmatched_requests_after_static_lookup() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("asset.txt"), "static asset")?;
    std::fs::create_dir_all(directory.path().join("api"))?;
    std::fs::create_dir_all(directory.path().join("ws"))?;
    std::fs::write(directory.path().join("api/existing.txt"), "api static")?;
    std::fs::write(directory.path().join("ws/socket.txt"), "ws static")?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/known", grouped_route)
                .post("/post-only", root_order)
                .fallback(fallback_echo)
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let known = router
        .clone()
        .oneshot(Request::get("/known").body(Body::empty())?)
        .await?;
    assert_eq!(known.status(), StatusCode::OK);
    assert_eq!(to_bytes(known.into_body(), usize::MAX).await?, "grouped");

    let method_mismatch = router
        .clone()
        .oneshot(Request::post("/known").body(Body::empty())?)
        .await?;
    assert_eq!(method_mismatch.status(), StatusCode::METHOD_NOT_ALLOWED);

    assert_fallback_static_resources(router.clone()).await?;

    // 当前 GET 没有普通路由时先尝试 GET fallback，再判断 POST alternate。
    let post_path_get = router
        .clone()
        .oneshot(Request::get("/post-only").body(Body::empty())?)
        .await?;
    assert_eq!(post_path_get.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(post_path_get.into_body(), usize::MAX).await?,
        "/post-only|missing|"
    );

    let fallback_options = router
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/not-found")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(fallback_options.status(), StatusCode::OK);
    assert_eq!(fallback_options.headers()[header::ALLOW], "GET,HEAD");
    assert!(
        to_bytes(fallback_options.into_body(), usize::MAX)
            .await?
            .is_empty()
    );

    let unknown_post = router
        .clone()
        .oneshot(Request::post("/api/not-found").body(Body::empty())?)
        .await?;
    assert_eq!(unknown_post.status(), StatusCode::METHOD_NOT_ALLOWED);

    let fallback = router
        .oneshot(
            Request::get("/api/not-found?source=client")
                .header("x-fallback-marker", "preserved")
                .body(Body::from("fallback body"))?,
        )
        .await?;
    assert_eq!(fallback.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(fallback.into_body(), usize::MAX).await?,
        "/api/not-found?source=client|preserved|fallback body"
    );
    Ok(())
}

#[tokio::test]
async fn grouped_fallback_is_prefix_scoped_and_uses_group_middleware() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new()
                    .prefix("/pages")
                    .middleware(axum::middleware::from_fn(mark_route_group)),
                |pages| pages.fallback(fallback_echo),
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    let grouped = router
        .clone()
        .oneshot(Request::get("/pages/missing").body(Body::empty())?)
        .await?;
    assert_eq!(grouped.status(), StatusCode::OK);
    assert_eq!(grouped.headers()["x-route-group"], "api");
    assert_eq!(
        to_bytes(grouped.into_body(), usize::MAX).await?,
        "/pages/missing|missing|"
    );

    let outside = router
        .clone()
        .oneshot(Request::get("/outside").body(Body::empty())?)
        .await?;
    assert_eq!(outside.status(), StatusCode::NOT_FOUND);
    assert!(!outside.headers().contains_key("x-route-group"));

    let api_missing = router
        .oneshot(Request::get("/api/not-found").body(Body::empty())?)
        .await?;
    assert_eq!(api_missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        api_missing.headers()[header::CONTENT_TYPE],
        "application/json"
    );
    Ok(())
}

#[tokio::test]
async fn explicit_fallback_runs_when_api_status_mode_maps_missing_to_ok() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::AlwaysOk;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| routes.fallback(fallback_echo)));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    for path in ["/missing", "/api/missing"] {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await?,
            format!("{path}|missing|")
        );
    }
    Ok(())
}

#[tokio::test]
async fn duplicate_fallbacks_fail_router_build_without_panicking() -> TestResult {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes
                .fallback(fallback_echo)
                .merge(openrust::http::Routes::new().fallback(fallback_echo))
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let Err(error) = build_router(&app) else {
        return Err(std::io::Error::other("duplicate fallback must be rejected").into());
    };
    assert_eq!(error.code(), "http_fallback_duplicate");

    let duplicate_groups = Routes::new()
        .group(RouteGroup::new().prefix("/pages"), |routes| {
            routes.fallback(fallback_echo)
        })
        .group(RouteGroup::new().prefix("/pages"), |routes| {
            routes.fallback(fallback_echo)
        });
    let Err(error) = duplicate_groups.into_router() else {
        return Err(std::io::Error::other("duplicate group fallback must be rejected").into());
    };
    assert_eq!(error.code(), "http_fallback_duplicate");

    let parameter_group = Routes::new().group(RouteGroup::new().prefix("/{id}"), |routes| {
        routes.fallback(fallback_echo)
    });
    let Err(error) = parameter_group.into_router() else {
        return Err(std::io::Error::other("parameterized fallback scope must be rejected").into());
    };
    assert_eq!(error.code(), "http_fallback_scope_unsupported");

    let merged_groups = Routes::new().merge(
        Routes::new()
            .group(RouteGroup::new().prefix("/pages"), |routes| {
                routes.fallback(fallback_echo)
            })
            .group(RouteGroup::new().prefix("/other"), |routes| {
                routes.fallback(fallback_echo)
            }),
    );
    let Err(error) = merged_groups
        .group(RouteGroup::new().prefix("/pages"), |routes| {
            routes.fallback(fallback_echo)
        })
        .into_router()
    else {
        return Err(std::io::Error::other("merged duplicate fallback must be rejected").into());
    };
    assert_eq!(error.code(), "http_fallback_duplicate");
    Ok(())
}

#[tokio::test]
async fn root_target_supports_internal_forward_and_redirect() -> TestResult {
    let mut forward = FrameworkConfig::default();
    forward.server.root_target = "forward:/api/greet".to_owned();
    let response = router_with_exact_config(forward)
        .await?
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "hello");

    let mut redirect = FrameworkConfig::default();
    redirect.server.root_target = "redirect:https://example.com/home".to_owned();
    let response = router_with_exact_config(redirect)
        .await?
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response.headers()[header::LOCATION],
        "https://example.com/home"
    );
    Ok(())
}

#[tokio::test]
async fn site_root_target_precedes_static_index_and_user_fallback() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("index.html"),
        "unselected static index",
    )?;
    std::fs::write(directory.path().join("chosen.html"), "chosen file")?;
    std::fs::write(directory.path().join("asset.txt"), "static asset")?;
    for (target, expected) in [
        ("file:/chosen.html", "chosen file"),
        ("forward:/forwarded", "root:/forwarded?source=client:"),
        ("forward:/missing", "domain fallback"),
        ("forward:/asset.txt", "static asset"),
        ("forward:/", "unselected static index"),
        ("redirect:/destination", ""),
    ] {
        let mut config = FrameworkConfig::default();
        config.server.public_dir = directory.path().to_owned();
        config.server.root_target = target.into();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(|routes| {
                routes
                    .get("/forwarded", root_order)
                    .get("/known", grouped_route)
                    .fallback(root_fallback_marker)
                    .group(RouteGroup::new().domain("a.example.test"), |domain| {
                        domain.fallback(domain_fallback_marker)
                    })
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        assert_site_root_target(router, target, expected).await?;
    }
    Ok(())
}

async fn assert_site_root_target(router: axum::Router, target: &str, expected: &str) -> TestResult {
    for method in ["GET", "HEAD"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/?source=client")
                    .header(header::HOST, "a.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if target.starts_with("redirect:") {
                StatusCode::TEMPORARY_REDIRECT
            } else {
                StatusCode::OK
            }
        );
        assert!(response.headers().contains_key("x-request-id"));
        if target.starts_with("redirect:") {
            assert_eq!(response.headers()[header::LOCATION], "/destination");
        }
        if !target.starts_with("redirect:") {
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                expected.len().to_string()
            );
        }
        assert_eq!(
            to_bytes(response.into_body(), 4096).await?,
            if method == "HEAD" { "" } else { expected },
            "{target} {method}"
        );
    }
    for (path, host, expected) in [
        ("/asset.txt", "a.example.test", "static asset"),
        ("/missing", "a.example.test", "domain fallback"),
        ("/missing", "b.example.test", "root fallback"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
    }
    let response = router
        .oneshot(
            Request::post("/known")
                .header(header::HOST, "a.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    Ok(())
}

#[cfg(feature = "view")]
#[tokio::test]
async fn site_root_view_uses_runtime_name_before_static_and_fallback() -> TestResult {
    let directory = tempfile::tempdir()?;
    let public = directory.path().join("public");
    let views = directory.path().join("views");
    std::fs::create_dir(&public)?;
    std::fs::create_dir(&views)?;
    std::fs::write(public.join("index.html"), "unselected static index")?;
    std::fs::write(
        views.join("index.html"),
        "{{ app_name }}|{{ errors.count() }}",
    )?;
    for explicit_root in [false, true] {
        let mut config = FrameworkConfig::default();
        config.app.name = "configured-application & runtime".into();
        config.server.public_dir = public.clone();
        config.server.root_target = "view:index.html".into();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
            .merge(figment::providers::Serialized::defaults(
                serde_json::json!({
                    "view":{"directory":views,"auto_reload":false}
                }),
            ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(openrust::view::ViewServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                let routes = routes.fallback(root_fallback_marker).group(
                    RouteGroup::new().domain("a.example.test"),
                    |domain| {
                        domain
                            .get("/", first_domain_marker)
                            .fallback(domain_fallback_marker)
                    },
                );
                if explicit_root {
                    routes.get("/", second_domain_marker)
                } else {
                    routes
                }
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for (host, expected) in [
            ("a.example.test", "first domain"),
            (
                "b.example.test",
                if explicit_root {
                    "second domain"
                } else {
                    "configured-application &amp; runtime|0"
                },
            ),
        ] {
            for method in ["GET", "HEAD"] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri("/")
                            .header(header::HOST, host)
                            .body(Body::empty())?,
                    )
                    .await?;
                assert_eq!(response.status(), StatusCode::OK);
                assert_root_representation_headers(
                    &response,
                    expected,
                    host == "b.example.test" && !explicit_root,
                );
                assert_eq!(
                    to_bytes(response.into_body(), 4096).await?,
                    if method == "HEAD" { "" } else { expected },
                    "{host} {method}"
                );
            }
        }
    }
    Ok(())
}

#[cfg(feature = "view")]
fn assert_root_representation_headers(response: &Response, expected: &str, html: bool) {
    let content_type = if html {
        "text/html; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    };
    assert_eq!(response.headers()[header::CONTENT_TYPE], content_type);
    // HTTP 不要求长度头必有；一旦提供则须描述 GET 表示，而不是空 HEAD body。
    if let Some(length) = response.headers().get(header::CONTENT_LENGTH) {
        assert_eq!(length.as_bytes(), expected.len().to_string().as_bytes());
    }
}

#[tokio::test]
async fn site_root_method_miss_keeps_fallback_and_allow() -> TestResult {
    let directory = tempfile::tempdir()?;
    for has_fallback in [false, true] {
        let mut config = FrameworkConfig::default();
        config.server.public_dir = directory.path().to_owned();
        config.server.root_target = "redirect:/unused".into();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                let routes = routes.post("/", root_order);
                if has_fallback {
                    routes.fallback(root_fallback_marker)
                } else {
                    routes
                }
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for method in ["GET", "HEAD", "PUT", "OPTIONS"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/")
                        .body(Body::empty())?,
                )
                .await?;
            assert!(!response.headers().contains_key(header::LOCATION));
            let expected = if method == "OPTIONS" || (has_fallback && method != "PUT") {
                StatusCode::OK
            } else {
                StatusCode::METHOD_NOT_ALLOWED
            };
            assert_eq!(response.status(), expected, "{has_fallback} {method}");
            if method == "OPTIONS" || expected == StatusCode::METHOD_NOT_ALLOWED {
                let allow = response.headers()[header::ALLOW].to_str()?;
                assert!(allow.split(',').any(|value| value.trim() == "POST"));
                assert_eq!(
                    allow.split(',').any(|value| value.trim() == "GET"),
                    has_fallback
                );
            }
            if method == "HEAD" || method == "OPTIONS" {
                assert!(to_bytes(response.into_body(), 4096).await?.is_empty());
            } else if has_fallback && method == "GET" {
                assert_eq!(to_bytes(response.into_body(), 4096).await?, "root fallback");
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn site_root_forward_fallback_preserves_external_url_and_route_capture() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    config.server.root_target = "forward:/nested/missing".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new().domain("a.example.test").prefix("/nested"),
                |domain| {
                    domain.fallback(
                        |info: openrust::http::RequestInfo, request: Request| async move {
                            let captured = info
                                .route()?
                                .and_then(|route| {
                                    route
                                        .parameter("fallbackPlaceholder")
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_owned)
                                })
                                .unwrap_or_default();
                            Ok::<_, openrust::Error>(format!(
                                "{}|{}|{captured}",
                                request.uri(),
                                info.full_url()?
                            ))
                        },
                    )
                },
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let response = build_router(&app)?
        .oneshot(Request::get("http://a.example.test/?q=client").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 4096).await?,
        "http://a.example.test/nested/missing?q=client|http://a.example.test/?q=client|missing"
    );
    Ok(())
}

#[tokio::test]
async fn site_head_responses_keep_explicit_status_headers_and_errors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    config.server.root_target = "file:/missing.html".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.head("/head-only", || async {
                (
                    StatusCode::ACCEPTED,
                    [("x-head-proof", "preserved"), ("content-length", "777")],
                    "HEAD-specific body",
                )
            })
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let explicit = router
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/head-only")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(explicit.status(), StatusCode::ACCEPTED);
    assert_eq!(explicit.headers()["x-head-proof"], "preserved");
    assert_eq!(explicit.headers()[header::CONTENT_LENGTH], "777");
    assert!(to_bytes(explicit.into_body(), 4096).await?.is_empty());
    let get = router
        .clone()
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    let head = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get.status(), StatusCode::NOT_FOUND);
    assert_eq!(head.status(), get.status());
    assert_eq!(
        head.headers()[header::CONTENT_TYPE],
        get.headers()[header::CONTENT_TYPE]
    );
    assert_eq!(
        head.headers().get(header::CACHE_CONTROL),
        get.headers().get(header::CACHE_CONTROL)
    );
    assert_eq!(json_body(get).await["code"], "not_found");
    assert!(to_bytes(head.into_body(), 4096).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn site_default_index_keeps_static_then_user_fallback() -> TestResult {
    for has_index in [false, true] {
        let directory = tempfile::tempdir()?;
        if has_index {
            std::fs::write(directory.path().join("index.html"), "default static index")?;
        }
        let mut config = FrameworkConfig::default();
        config.server.public_dir = directory.path().to_owned();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(|routes| {
                routes.fallback(root_fallback_marker)
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for method in ["GET", "HEAD"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/")
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            let expected = if method == "HEAD" {
                ""
            } else if has_index {
                "default static index"
            } else {
                "root fallback"
            };
            assert_eq!(to_bytes(response.into_body(), 4096).await?, expected);
        }
    }
    Ok(())
}

#[tokio::test]
async fn standalone_root_fallback_ignores_site_target() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut config = FrameworkConfig::default();
    config.server.public_dir = directory.path().to_owned();
    config.server.root_target = "redirect:/unused".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder.provider(HttpServiceProvider);
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = Routes::new()
        .fallback(root_fallback_marker)
        .into_router()?
        .with_state(app.state().clone());
    let response = router
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::LOCATION));
    assert_eq!(to_bytes(response.into_body(), 4096).await?, "root fallback");
    Ok(())
}

#[tokio::test]
async fn site_root_handler_error_status_is_terminal() -> TestResult {
    for status in [StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED] {
        let mut config = FrameworkConfig::default();
        config.server.root_target = "redirect:/unused".into();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(TestRoutes::new(move |routes| {
                routes
                    .get("/", move || async move { status })
                    .fallback(root_fallback_marker)
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let response = build_router(&app)?
            .oneshot(Request::get("/").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), status);
        assert!(!response.headers().contains_key(header::LOCATION));
    }
    Ok(())
}

#[tokio::test]
async fn invalid_root_target_is_rejected_during_router_build() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.root_target = "redirect:javascript:alert(1)".to_owned();
    let error = router_with_exact_config(config)
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("unsafe redirect scheme must be rejected"))?;
    assert!(error.to_string().contains("server.root_target"));
    Ok(())
}

#[tokio::test]
async fn static_fallback_does_not_capture_api_or_websocket_namespaces() -> TestResult {
    let app = router().await?;
    for path in ["/api/unknown", "/ws/unknown"] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(json_body(response).await["code"], "not_found");
    }
    Ok(())
}

#[tokio::test]
async fn disabled_compression_ignores_accept_encoding() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.compression.enabled = false;
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/large")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())?,
        )
        .await?;
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    Ok(())
}

#[tokio::test]
async fn cors_is_disabled_by_default() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    Ok(())
}

#[tokio::test]
async fn cors_allows_listed_origin_and_preflight_with_request_id() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    config.server.cors.expose_headers = vec!["x-total-count".into()];
    let router = router_with_config(config).await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_EXPOSE_HEADERS),
        Some(&"x-total-count".parse()?)
    );

    let preflight = router
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header(header::ORIGIN, "https://example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(preflight.status(), StatusCode::OK);
    assert!(preflight.headers().contains_key("x-request-id"));
    assert_eq!(
        preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS),
        Some(&"GET,POST,PUT,PATCH,DELETE,OPTIONS".parse()?)
    );
    assert_eq!(
        preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS),
        Some(&"content-type,authorization,x-request-id".parse()?)
    );
    assert_eq!(
        preflight.headers().get(header::ACCESS_CONTROL_MAX_AGE),
        Some(&"600".parse()?)
    );
    assert!(
        to_bytes(preflight.into_body(), usize::MAX)
            .await?
            .is_empty()
    );

    let denied = router
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://other.example")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !denied
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    Ok(())
}

#[tokio::test]
async fn cors_preflight_rejects_unlisted_origin() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/health")
                .header(header::ORIGIN, "https://other.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .body(Body::empty())?,
        )
        .await?;
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn cors_origin_error_is_wrapped_outside_preflight() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/not-found")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
    let body = json_body(response).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "not_found");
    assert!(body.get("data").is_none());
    assert_eq!(body["request_id"], request_id);
    Ok(())
}

#[tokio::test]
async fn cors_timeout_retains_allow_origin() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/api/slow")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"https://example.com".parse()?)
    );
    assert_eq!(json_body(response).await["code"], "request_timeout");
    Ok(())
}

#[tokio::test]
async fn cors_wildcard_and_credentials_have_expected_headers() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["*".into()];
    let response = router_with_config(config.clone())
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&"*".parse()?)
    );

    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    config.server.cors.allow_credentials = true;
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
        Some(&"true".parse()?)
    );
    Ok(())
}

#[tokio::test]
async fn invalid_cors_config_returns_keyed_errors() {
    let cases = [
        (vec![], false, "server.cors.allowed_origins"),
        (
            vec!["*", "https://example.com"],
            false,
            "server.cors.allowed_origins",
        ),
        (vec!["*"], true, "server.cors.allow_credentials"),
        (
            vec!["https://example.com", "bad\norigin"],
            false,
            "server.cors.allowed_origins[1]",
        ),
    ];
    for (origins, credentials, key) in cases {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = origins.into_iter().map(str::to_owned).collect();
        config.server.cors.allow_credentials = credentials;
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains(key), "{error}");
    }
}

#[tokio::test]
async fn invalid_cors_origin_reports_escaped_value() {
    for origin in [
        "bad\norigin",
        "https://example.com/",
        "https://example.com/path",
        "https://example.com?x=1",
        "https://example.com:bad",
        "example.com",
    ] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec![origin.into()];
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.allowed_origins[0]: invalid origin {origin:?}"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn cors_wildcards_with_credentials_return_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        config.server.cors.allow_credentials = true;
        match key {
            "allowed_methods" => config.server.cors.allowed_methods = vec!["*".into()],
            "allowed_headers" => config.server.cors.allowed_headers = vec!["*".into()],
            _ => config.server.cors.expose_headers = vec!["*".into()],
        }
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[0]: cannot be \"*\" when server.cors.allow_credentials = true"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn cors_mixed_wildcards_return_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        match key {
            "allowed_methods" => {
                config.server.cors.allowed_methods = vec!["GET".into(), "*".into()];
            }
            "allowed_headers" => {
                config.server.cors.allowed_headers = vec!["content-type".into(), "*".into()];
            }
            _ => config.server.cors.expose_headers = vec!["x-total-count".into(), "*".into()],
        }
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[1]: \"*\" cannot be mixed with concrete values"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn invalid_cors_methods_and_headers_return_keyed_errors() {
    for key in ["allowed_methods", "allowed_headers", "expose_headers"] {
        let mut config = FrameworkConfig::default();
        config.server.cors.enabled = true;
        config.server.cors.allowed_origins = vec!["https://example.com".into()];
        let invalid_value = if key == "allowed_methods" {
            config.server.cors.allowed_methods = vec!["BAD METHOD".into()];
            "BAD METHOD"
        } else if key == "allowed_headers" {
            config.server.cors.allowed_headers = vec!["bad header".into()];
            "bad header"
        } else {
            config.server.cors.expose_headers = vec!["bad header".into()];
            "bad header"
        };
        let error = router_with_config(config)
            .await
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(&format!(
                "server.cors.{key}[0]: invalid value {invalid_value:?}"
            )),
            "{error}"
        );
    }
}

#[tokio::test]
async fn disabled_cors_ignores_other_invalid_fields() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.allowed_origins = vec!["bad\norigin".into()];
    let response = router_with_config(config)
        .await?
        .oneshot(
            Request::get("/health")
                .header(header::ORIGIN, "https://example.com")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(())
}

#[tokio::test]
async fn json_syntax_is_400_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{broken"))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "bad_request");
    Ok(())
}

#[tokio::test]
async fn json_content_type_is_415_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::post("/api/raw").body(Body::from("{}"))?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(json_body(response).await["code"], "unsupported_media_type");
    Ok(())
}

#[tokio::test]
async fn json_field_type_is_422_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":"hello","pages":"wrong"}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|detail| detail.contains("pages"))
    );
    assert!(body["errors"]["pages"].is_array(), "{body}");
    Ok(())
}

#[tokio::test]
async fn oversized_json_is_413_envelope() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(vec![b' '; 3_000_000]))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json_body(response).await["code"], "payload_too_large");
    Ok(())
}

#[tokio::test]
async fn path_and_query_parse_errors_are_400_envelopes() -> TestResult {
    for uri in ["/api/items/nope", "/api/search?title=x&pages=nope"] {
        let response = router()
            .await?
            .oneshot(Request::get(uri).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["code"], "bad_request");
    }
    Ok(())
}

#[tokio::test]
async fn unknown_route_is_404_envelope_with_request_id() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/unknown").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body = json_body(response).await;
    assert_eq!(body["code"], "not_found");
    assert!(is_utc_rfc3339(&body["timestamp"]));
    Ok(())
}

#[tokio::test]
async fn wrong_method_is_405_envelope_with_allow() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::post("/api/greet").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(response.headers().contains_key(header::ALLOW));
    assert_eq!(json_body(response).await["code"], "method_not_allowed");
    Ok(())
}

#[tokio::test]
async fn handler_404_keeps_its_detail() -> TestResult {
    let response = router()
        .await?
        .oneshot(Request::get("/api/own-404").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = json_body(response).await;
    assert_eq!(body["message"], "my resource is absent");
    Ok(())
}

#[tokio::test]
async fn cors_applies_to_fallback_envelopes() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.cors.enabled = true;
    config.server.cors.allowed_origins = vec!["https://example.com".into()];
    let router = router_with_config(config).await?;
    for (method, uri, status) in [
        ("GET", "/nope", StatusCode::NOT_FOUND),
        ("POST", "/api/greet", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::ORIGIN, "https://example.com")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://example.com"
        );
        assert!(response.headers().contains_key("x-request-id"));
    }
    Ok(())
}

#[tokio::test]
async fn missing_json_field_is_422_with_field_error() -> TestResult {
    let response = router()
        .await?
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "validation_failed");
    assert!(body["errors"]["title"].is_array(), "{body}");
    Ok(())
}

#[tokio::test]
async fn always_ok_wraps_success_errors_and_fallbacks_with_matching_request_id() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::AlwaysOk;
    let router = router_with_exact_config(config).await?;
    for (method, uri, expected_code) in [
        ("GET", "/api/page", "ok"),
        ("POST", "/api/created", "ok"),
        ("DELETE", "/api/no-content", "ok"),
        ("GET", "/nope", "not_found"),
        ("POST", "/api/greet", "method_not_allowed"),
        ("GET", "/api/boom", "internal_error"),
        ("GET", "/api/limited", "rate_limited"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-request-id", "client-123")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(response.headers()["x-request-id"], "client-123");
        if uri == "/api/created" {
            assert_eq!(response.headers()[header::LOCATION], "/api/items/42");
        }
        if uri == "/api/limited" {
            assert_eq!(response.headers()[header::RETRY_AFTER], "30");
        }
        if method == "POST" && uri == "/api/greet" {
            assert!(response.headers().contains_key(header::ALLOW));
        }
        let body = json_body(response).await;
        assert_eq!(body["code"], expected_code, "{uri}");
        assert_eq!(body["success"], expected_code == "ok", "{uri}");
        if expected_code == "ok" {
            assert!(body.get("request_id").is_none(), "{uri}");
            assert!(body.get("timestamp").is_none(), "{uri}");
        } else {
            assert_eq!(body["request_id"], "client-123", "{uri}");
            assert!(is_utc_rfc3339(&body["timestamp"]), "{uri}");
        }
        if uri == "/api/no-content" || expected_code != "ok" {
            assert!(body.get("data").is_none(), "{uri}");
        }
        if uri == "/api/page" {
            assert_eq!(body["data"], serde_json::json!([1, 2]));
            assert_eq!(body["meta"]["total_pages"], 3);
        }
        if uri == "/api/boom" {
            assert_eq!(body["message"], "internal server error");
        }
    }
    Ok(())
}

#[tokio::test]
async fn http_execution_context_contains_request_id_budget_and_timeout_cancels_children()
-> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    let router = router_with_exact_config(config).await?;
    let response = router
        .clone()
        .oneshot(
            Request::get("/api/execution")
                .header("x-request-id", "scope-123")
                .body(Body::empty())?,
        )
        .await?;
    let body = json_body(response).await;
    assert_eq!(body["data"]["request_id"], "scope-123");
    assert_eq!(body["data"]["has_budget"], true);

    REQUEST_CANCELLED.store(false, Ordering::SeqCst);
    let response = router
        .oneshot(Request::get("/api/cancel").body(Body::empty())?)
        .await?;
    assert_eq!(json_body(response).await["code"], "request_timeout");
    tokio::time::timeout(Duration::from_millis(100), async {
        while !REQUEST_CANCELLED.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn semantic_keeps_created_error_and_rate_limit_statuses() -> TestResult {
    let router = router().await?;
    for (method, uri, expected) in [
        ("POST", "/api/created", StatusCode::CREATED),
        ("DELETE", "/api/no-content", StatusCode::OK),
        ("GET", "/nope", StatusCode::NOT_FOUND),
        ("POST", "/api/greet", StatusCode::METHOD_NOT_ALLOWED),
        ("GET", "/api/limited", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected, "{uri}");
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = json_body(response).await;
        if expected.is_success() {
            assert!(body.get("request_id").is_none());
            assert!(body.get("timestamp").is_none());
        } else {
            assert_eq!(body["request_id"], request_id);
            assert!(is_utc_rfc3339(&body["timestamp"]));
        }
    }
    Ok(())
}

#[tokio::test]
async fn always_ok_maps_extractor_errors_and_timeout_to_envelopes() -> TestResult {
    let mut config = FrameworkConfig::default();
    config.server.request_timeout_secs = 1;
    config.api.http_status = openrust::config::HttpStatusMode::AlwaysOk;
    let router = router_with_exact_config(config).await?;
    let cases = [
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{bad"))?,
            "bad_request",
        ),
        (
            Request::post("/api/raw").body(Body::from("{}"))?,
            "unsupported_media_type",
        ),
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
            "validation_failed",
        ),
        (
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!("\"{}\"", "x".repeat(2_100_000))))?,
            "payload_too_large",
        ),
        (
            Request::get("/api/slow").body(Body::empty())?,
            "request_timeout",
        ),
    ];
    for (request, code) in cases {
        let response = router.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK, "{code}");
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        let body = json_body(response).await;
        assert_eq!(body["request_id"], request_id, "{code}");
        assert_eq!(body["code"], code);
        assert_eq!(body["success"], false);
    }
    Ok(())
}

#[tokio::test]
async fn default_status_mode_uses_semantic_http_codes() -> TestResult {
    assert_eq!(
        FrameworkConfig::default().api.http_status,
        openrust::config::HttpStatusMode::Semantic
    );
    let router = router_with_exact_config(FrameworkConfig::default()).await?;
    for (method, uri, expected_status, expected_code) in [
        ("POST", "/api/created", StatusCode::CREATED, "ok"),
        ("GET", "/nope", StatusCode::NOT_FOUND, "not_found"),
        (
            "POST",
            "/api/greet",
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
        (
            "GET",
            "/api/boom",
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        ),
        (
            "GET",
            "/api/limited",
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
        ),
        (
            "POST",
            "/api/raw",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected_status, "{uri}");
        let body = json_body(response).await;
        assert_eq!(body["success"], expected_status.is_success(), "{uri}");
        assert_eq!(body["code"], expected_code, "{uri}");
    }
    let validation = router
        .oneshot(
            Request::post("/api/raw")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"pages":1}"#))?,
        )
        .await?;
    assert_eq!(validation.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(validation).await;
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "validation_failed");
    assert!(body["errors"]["title"].is_array());
    Ok(())
}

struct FailingI18nProvider {
    api_envelope: bool,
}
impl openrust::core::ServiceProvider for FailingI18nProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.scoped::<openrust::i18n::I18n, _>(|_| {
                Box::pin(async {
                    Err(openrust::Error::internal(std::io::Error::other(
                        "i18n service unavailable",
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            if self.api_envelope {
                app.exceptions(|handler| {
                    handler.api_envelope_when(|_, _| true);
                })?;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn i18n_resolution_errors_keep_request_status_mode_and_id() -> TestResult {
    for (mode, expected_status) in [
        (
            openrust::config::HttpStatusMode::Semantic,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (openrust::config::HttpStatusMode::AlwaysOk, StatusCode::OK),
    ] {
        let mut config = FrameworkConfig::default();
        config.api.http_status = mode;
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config))
            .merge(figment::Figment::new());
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(FailingI18nProvider { api_envelope: true });
        let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
        let response = build_router(&app)?
            .oneshot(
                Request::get("/unreachable")
                    .header("x-request-id", "i18n-resolution-failure")
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), expected_status);
        let request_id = response.headers()["x-request-id"].to_str()?.to_owned();
        assert_eq!(request_id, "i18n-resolution-failure");
        let body = json_body(response).await;
        assert_eq!(body["request_id"], request_id);
        assert_eq!(body["code"], "service_initialization_failed");
    }
    Ok(())
}

// 捕获 next 之前失败时默认协商响应遗漏 request-id 或 HEAD 正文的回归。
#[tokio::test]
async fn early_i18n_errors_keep_default_negotiation_and_request_id() -> TestResult {
    let config = FrameworkConfig::default();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(FailingI18nProvider {
            api_envelope: false,
        });
    let app = openrust::App::from_provider_runtime(builder.start(sources).await?)?;
    let router = build_router(&app)?;
    for (method, accept, expected_type) in [
        ("GET", None, "text/html"),
        ("GET", Some("text/html"), "text/html"),
        ("GET", Some("application/json"), "application/json"),
        ("HEAD", None, "text/html"),
        ("HEAD", Some("application/json"), "application/json"),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri("/unreachable")
            .header("x-request-id", "default-i18n-failure");
        if let Some(accept) = accept {
            request = request.header(header::ACCEPT, accept);
        }
        let response = router.clone().oneshot(request.body(Body::empty())?).await?;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()["x-request-id"], "default-i18n-failure");
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()?
                .starts_with(expected_type)
        );
        let body = to_bytes(response.into_body(), 4096).await?;
        if method == "HEAD" {
            assert!(body.is_empty());
        } else if expected_type == "application/json" {
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            assert_eq!(value["message"], "internal server error");
            assert!(value.get("code").is_none());
        } else {
            assert!(String::from_utf8_lossy(&body).contains("internal server error"));
        }
    }
    Ok(())
}

struct TestInstance<T: Send + Sync + 'static>(std::sync::Arc<T>);
impl<T: Send + Sync + 'static> openrust::core::ServiceProvider for TestInstance<T> {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(std::sync::Arc::clone(&self.0)).await?;
            Ok(())
        })
    }
}

struct TestRoutes<F>(std::sync::Arc<F>);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes> TestRoutes<F> {
    fn new(routes: F) -> Self {
        Self(std::sync::Arc::new(routes))
    }
}
impl<F> openrust::core::ServiceProvider for TestRoutes<F>
where
    F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + 'static,
{
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let routes = std::sync::Arc::clone(&self.0);
        Box::pin(async move {
            app.exceptions(|handler| {
                handler.api_envelope_when(|_, _| true);
            })?;
            app.routes(move |value| routes(value))?;
            Ok(())
        })
    }
}

struct PrecognitionProbe;

struct RejectAfterValidation;

impl<S> axum::extract::FromRequestParts<S> for RejectAfterValidation
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        if let Some(calls) = parts.extensions.get::<Arc<AtomicUsize>>() {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        std::future::ready(Err(StatusCode::IM_A_TEAPOT))
    }
}

impl<S> axum::extract::FromRequestParts<S> for PrecognitionProbe
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let info = <openrust::http::RequestInfo as axum::extract::FromRequestParts<S>>::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if let Some(events) = parts.extensions.get::<Arc<Mutex<Vec<(bool, bool)>>>>()
            && let Ok(mut events) = events.lock()
        {
            events.push((
                info.is_attempting_precognition()
                    .map_err(|_| StatusCode::BAD_REQUEST)?,
                info.is_precognitive(),
            ));
        }
        if parts.headers.contains_key("x-reject-before-body") {
            return Err(StatusCode::IM_A_TEAPOT);
        }
        Ok(Self)
    }
}

async fn add_existing_vary(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().append(
        header::VARY,
        axum::http::HeaderValue::from_static("Accept-Encoding"),
    );
    response
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "逐个真实请求断言确认、拒绝、无中间件与并发隔离的连续状态"
)]
async fn precognition_confirms_only_opted_in_routes_after_extraction() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let routes_calls = Arc::clone(&calls);
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(move |routes| {
            let managed_calls = Arc::clone(&routes_calls);
            let plain_calls = Arc::clone(&routes_calls);
            routes
                .group(
                    RouteGroup::new()
                        .middleware_alias("precognitive")
                        .middleware(axum::middleware::from_fn(add_existing_vary)),
                    move |routes| {
                        let managed_calls = Arc::clone(&managed_calls);
                        routes.post(
                        "/precognitive",
                        move |PrecognitionProbe, axum::Json(_): axum::Json<serde_json::Value>| {
                            let calls = Arc::clone(&managed_calls);
                            async move {
                                calls.fetch_add(1, Ordering::SeqCst);
                                "called"
                            }
                        },
                    )
                    },
                )
                .post(
                    "/plain",
                    move |PrecognitionProbe, axum::Json(_): axum::Json<serde_json::Value>| {
                        let calls = Arc::clone(&plain_calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            "called"
                        }
                    },
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    let events = Arc::new(Mutex::new(Vec::new()));

    let mut request = Request::post("/precognitive")
        .header("precognition", "true")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;
    request.extensions_mut().insert(Arc::clone(&events));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers()["precognition"], "true");
    assert_eq!(response.headers()["precognition-success"], "true");
    assert!(
        response.headers()[header::VARY]
            .to_str()?
            .contains("Accept-Encoding")
    );
    assert!(
        response.headers()[header::VARY]
            .to_str()?
            .contains("Precognition")
    );
    assert!(to_bytes(response.into_body(), 1024).await?.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        *events
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?,
        vec![(true, true)]
    );

    let mut request = Request::post("/precognitive")
        .header("precognition", "true")
        .header("x-reject-before-body", "yes")
        .header("content-type", "application/json")
        .body(Body::from("bad"))?;
    request.extensions_mut().insert(Arc::clone(&events));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
    assert_eq!(response.headers()["precognition"], "true");
    assert!(!response.headers().contains_key("precognition-success"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let request = Request::post("/precognitive")
        .header("precognition", "true")
        .header("content-type", "application/json")
        .body(Body::from("bad"))?;
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!response.headers().contains_key("precognition-success"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut request = Request::post("/plain")
        .header("precognition", "true")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;
    request.extensions_mut().insert(Arc::clone(&events));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("precognition-success"));
    assert!(!response.headers().contains_key("precognition"));
    assert!(
        !response
            .headers()
            .get_all(header::VARY)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.contains("Precognition"))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?
            .last(),
        Some(&(true, false))
    );

    let confirmed_events = Arc::new(Mutex::new(Vec::<(bool, bool)>::new()));
    let ordinary_events = Arc::new(Mutex::new(Vec::<(bool, bool)>::new()));
    let mut confirmed_request = Request::post("/precognitive")
        .header("precognition", "true")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;
    confirmed_request
        .extensions_mut()
        .insert(Arc::clone(&confirmed_events));
    let mut ordinary_request = Request::post("/precognitive")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;
    ordinary_request
        .extensions_mut()
        .insert(Arc::clone(&ordinary_events));
    let (confirmed_response, ordinary_response) = tokio::join!(
        router.clone().oneshot(confirmed_request),
        router.clone().oneshot(ordinary_request),
    );
    assert_eq!(confirmed_response?.status(), StatusCode::NO_CONTENT);
    assert_eq!(ordinary_response?.status(), StatusCode::OK);
    assert_eq!(
        *confirmed_events
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?,
        vec![(true, true)]
    );
    assert_eq!(
        *ordinary_events
            .lock()
            .map_err(|_| std::io::Error::other("events poisoned"))?,
        vec![(false, false)]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let request = Request::post("/precognitive")
        .header("precognition", "TRUE")
        .header("content-type", "application/json")
        .body(Body::from("{}"))?;
    let response = router.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "同一受管理路由连续核对字段选择、完整验证与后续提取器短路"
)]
async fn validated_precognition_selects_json_and_stops_query_parts_extraction() -> TestResult {
    let business_calls = Arc::new(AtomicUsize::new(0));
    let route_calls = Arc::clone(&business_calls);
    let mut config = FrameworkConfig::default();
    config.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(move |routes| {
            let json_calls = Arc::clone(&route_calls);
            let plain_calls = Arc::clone(&route_calls);
            routes
                .group(
                    RouteGroup::new().middleware_alias("precognitive"),
                    move |routes| {
                        let json_calls = Arc::clone(&json_calls);
                        routes
                            .post(
                                "/validated-json",
                                move |Validated(Json(input)): Validated<Json<CreatePost>>| {
                                    let calls = Arc::clone(&json_calls);
                                    async move {
                                        calls.fetch_add(1, Ordering::SeqCst);
                                        Json(input.title)
                                    }
                                },
                            )
                            .get(
                                "/validated-query",
                                |Validated(Query(_input)): Validated<Query<CreatePost>>,
                                 _later: RejectAfterValidation,
                                 _request: Request| async move { "business" },
                            )
                    },
                )
                .post(
                    "/plain-validated-json",
                    move |Validated(Json(input)): Validated<Json<CreatePost>>| {
                        let calls = Arc::clone(&plain_calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Json(input.title)
                        }
                    },
                )
                .get(
                    "/plain-validated-query",
                    |Validated(Query(input)): Validated<Query<CreatePost>>, _request: Request| async move {
                        Json(input.title)
                    },
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;

    for (body, selected, expected_status, success) in [
        (r#"{"title":"good"}"#, "title", StatusCode::NO_CONTENT, true),
        (
            r#"{"title":"no"}"#,
            "title",
            StatusCode::UNPROCESSABLE_ENTITY,
            false,
        ),
        (r#"{"title":"good"}"#, "", StatusCode::NO_CONTENT, true),
    ] {
        let request = Request::post("/validated-json")
            .header("precognition", "true")
            .header("precognition-validate-only", selected)
            .header("content-type", "application/json")
            .body(Body::from(body))?;
        let response = router.clone().oneshot(request).await?;
        assert_eq!(response.status(), expected_status, "{body} / {selected}");
        assert_eq!(response.headers()["precognition"], "true");
        assert!(
            response.headers()[header::VARY]
                .to_str()?
                .contains("Precognition")
        );
        assert_eq!(
            response.headers().get("precognition-success").is_some(),
            success
        );
        if success {
            assert!(to_bytes(response.into_body(), 1024).await?.is_empty());
        } else {
            assert_eq!(json_body(response).await["code"], "validation_failed");
        }
    }
    assert_eq!(business_calls.load(Ordering::SeqCst), 0);

    let request = Request::post("/plain-validated-json")
        .header("precognition", "true")
        .header("precognition-validate-only", "title")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"title":"good"}"#))?;
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!response.headers().contains_key("precognition"));
    assert!(!response.headers().contains_key("precognition-success"));
    assert_eq!(business_calls.load(Ordering::SeqCst), 0);

    let request = Request::post("/plain-validated-json")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"title":"good","pages":2}"#))?;
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(business_calls.load(Ordering::SeqCst), 1);

    let response = router
        .clone()
        .oneshot(Request::get("/plain-validated-query?title=good&pages=2").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["data"], "good");
    let response = router
        .clone()
        .oneshot(Request::get("/plain-validated-query?title=no&pages=2").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(response).await["code"], "validation_failed");

    let later_calls = Arc::new(AtomicUsize::new(0));
    let mut request = Request::get("/validated-query?title=good")
        .header("precognition", "true")
        .header("precognition-validate-only", "title")
        .body(Body::empty())?;
    request.extensions_mut().insert(Arc::clone(&later_calls));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers()["precognition"], "true");
    assert_eq!(response.headers()["precognition-success"], "true");
    assert!(
        response.headers()[header::VARY]
            .to_str()?
            .contains("Precognition")
    );
    assert!(to_bytes(response.into_body(), 1024).await?.is_empty());
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);

    let mut request = Request::get("/validated-query?title=good&pages=2")
        .header("precognition", "true")
        .body(Body::empty())?;
    request.extensions_mut().insert(Arc::clone(&later_calls));
    let response = router.clone().oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::IM_A_TEAPOT);
    assert_eq!(response.headers()["precognition"], "true");
    assert!(!response.headers().contains_key("precognition-success"));
    assert_eq!(later_calls.load(Ordering::SeqCst), 1);

    let request = Request::get("/validated-query?title=no&pages=2")
        .header("precognition", "true")
        .header("precognition-validate-only", "title")
        .body(Body::empty())?;
    let response = router.oneshot(request).await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()["precognition"], "true");
    assert!(!response.headers().contains_key("precognition-success"));
    assert_eq!(later_calls.load(Ordering::SeqCst), 1);
    Ok(())
}

type IdentityFactory = dyn Fn(
        openrust::core::container::Resolver,
    ) -> openrust::BoxFuture<'static, openrust::Result<Arc<RequestIdentity>>>
    + Send
    + Sync;
struct IdentityProvider(std::sync::Arc<IdentityFactory>);
impl IdentityProvider {
    fn new(
        factory: impl Fn(
            openrust::core::container::Resolver,
        )
            -> openrust::BoxFuture<'static, openrust::Result<Arc<RequestIdentity>>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(factory))
    }
}
impl openrust::core::ServiceProvider for IdentityProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let factory = Arc::clone(&self.0);
            app.scoped::<RequestIdentity, _>(move |resolver| factory(resolver))
                .await?;
            Ok(())
        })
    }
}
struct SharedHttpLayer;
impl openrust::core::ServiceProvider for SharedHttpLayer {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.http_layer(axum::middleware::from_fn(add_shared_extension))?;
            Ok(())
        })
    }
}

// 以下 fixture 经生产 Provider/Router 装配后绑定真实 TCP listener；不以 oneshot 代替断连。
type NetworkResult<T> = Result<T, Box<dyn std::error::Error>>;

struct NetworkScopedResource(usize);
struct NetworkScopedIdentity(usize);
struct NetworkScopeProbe {
    next: AtomicUsize,
    cleanup_starts: AtomicUsize,
    cleanup_finishes: AtomicUsize,
    identity_barrier: tokio::sync::Barrier,
    release: tokio::sync::Semaphore,
    started: tokio::sync::mpsc::UnboundedSender<usize>,
    completed: tokio::sync::mpsc::UnboundedSender<usize>,
    scopes: tokio::sync::mpsc::UnboundedSender<Arc<Scope>>,
}
struct NetworkScopeEvents {
    started: tokio::sync::mpsc::UnboundedReceiver<usize>,
    completed: tokio::sync::mpsc::UnboundedReceiver<usize>,
    scopes: tokio::sync::mpsc::UnboundedReceiver<Arc<Scope>>,
}
struct NetworkScopeProvider(Arc<NetworkScopeProbe>);
impl openrust::core::ServiceProvider for NetworkScopeProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut openrust::core::RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(Arc::clone(&self.0)).await?;
            app.scoped::<NetworkScopedResource, _>(|resolver| {
                Box::pin(async move {
                    let probe = resolver.resolve::<NetworkScopeProbe>().await?;
                    let id = probe.next.fetch_add(1, Ordering::SeqCst) + 1;
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            probe.cleanup_starts.fetch_add(1, Ordering::SeqCst);
                            probe.started.send(id).map_err(openrust::Error::internal)?;
                            let permit = probe
                                .release
                                .acquire()
                                .await
                                .map_err(openrust::Error::internal)?;
                            permit.forget();
                            probe.cleanup_finishes.fetch_add(1, Ordering::SeqCst);
                            probe
                                .completed
                                .send(id)
                                .map_err(openrust::Error::internal)?;
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(NetworkScopedResource(id)))
                })
            })
            .await?;
            app.scoped::<NetworkScopedIdentity, _>(|resolver| {
                Box::pin(async move {
                    let probe = resolver.resolve::<NetworkScopeProbe>().await?;
                    Ok(Arc::new(NetworkScopedIdentity(
                        probe.next.fetch_add(1, Ordering::SeqCst) + 1,
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/scope-stream", network_scope_stream)
                    .get("/scope-sse", network_scope_sse)
                    .get("/scope-identity", network_scope_identity)
            })?;
            Ok(())
        })
    }
}

async fn network_scope_stream(
    Inject(resource): Inject<NetworkScopedResource>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
    Extension(scope): Extension<Arc<Scope>>,
) -> openrust::Result<Response> {
    probe
        .scopes
        .send(scope)
        .map_err(openrust::Error::internal)?;
    let first = futures_util::stream::once(async move {
        let body_resource = state
            .resolve::<NetworkScopedResource>()
            .await
            .map_err(std::io::Error::other)?;
        if !Arc::ptr_eq(&resource, &body_resource) {
            return Err(std::io::Error::other(
                "stream body resolved a different request scope",
            ));
        }
        Ok::<_, std::io::Error>(bytes::Bytes::from(format!("scope={}\n", resource.0)))
    });
    Ok(Body::from_stream(first.chain(futures_util::stream::pending())).into_response())
}

async fn network_scope_sse(
    Inject(resource): Inject<NetworkScopedResource>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
    Extension(scope): Extension<Arc<Scope>>,
) -> openrust::Result<Response> {
    use axum::response::sse::{Event, Sse};
    probe
        .scopes
        .send(scope)
        .map_err(openrust::Error::internal)?;
    let first = futures_util::stream::once(async move {
        let body_resource = state
            .resolve::<NetworkScopedResource>()
            .await
            .map_err(std::io::Error::other)?;
        if !Arc::ptr_eq(&resource, &body_resource) {
            return Err(std::io::Error::other(
                "SSE body resolved a different request scope",
            ));
        }
        Ok::<_, std::io::Error>(Event::default().data(format!("scope={}", resource.0)))
    });
    Ok(Sse::new(first.chain(futures_util::stream::pending())).into_response())
}

async fn network_scope_identity(
    Inject(identity): Inject<NetworkScopedIdentity>,
    Inject(probe): Inject<NetworkScopeProbe>,
    axum::extract::State(state): axum::extract::State<openrust::AppState>,
) -> openrust::Result<axum::Json<serde_json::Value>> {
    let _ = probe.identity_barrier.wait().await;
    let again = state.resolve::<NetworkScopedIdentity>().await?;
    let request_id = ExecutionContext::current()
        .and_then(|context| context.metadata().request_id().map(str::to_owned));
    let plain = tokio::spawn(async move {
        let inherited = ExecutionContext::current().is_some();
        let root = state.resolve::<NetworkScopedIdentity>().await?;
        Ok::<_, openrust::Error>((inherited, root.0))
    })
    .await
    .map_err(openrust::Error::internal)??;
    Ok(axum::Json(serde_json::json!({
        "id":identity.0, "again":again.0, "request_id":request_id,
        "plain_inherited":plain.0, "plain_id":plain.1,
    })))
}

struct NetworkScopeServer {
    app: Arc<openrust::App>,
    address: std::net::SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl NetworkScopeServer {
    async fn start() -> NetworkResult<(Self, Arc<NetworkScopeProbe>, NetworkScopeEvents)> {
        let (started, started_rx) = tokio::sync::mpsc::unbounded_channel();
        let (completed, completed_rx) = tokio::sync::mpsc::unbounded_channel();
        let (scopes, scopes_rx) = tokio::sync::mpsc::unbounded_channel();
        let probe = Arc::new(NetworkScopeProbe {
            next: AtomicUsize::new(0),
            cleanup_starts: AtomicUsize::new(0),
            cleanup_finishes: AtomicUsize::new(0),
            identity_barrier: tokio::sync::Barrier::new(2),
            release: tokio::sync::Semaphore::new(0),
            started,
            completed,
            scopes,
        });
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(NetworkScopeProvider(Arc::clone(&probe)));
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let app = Arc::new(openrust::App::from_provider_runtime(
            builder.start(sources).await?,
        )?);
        let router = build_router(&app)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (stop, signal) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = signal.await;
                })
                .await
        });
        Ok((
            Self {
                app,
                address,
                stop,
                task,
            },
            probe,
            NetworkScopeEvents {
                started: started_rx,
                completed: completed_rx,
                scopes: scopes_rx,
            },
        ))
    }
    async fn finish(self) -> NetworkResult<()> {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(5), self.task).await???;
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        Ok(())
    }
}

async fn open_scope_stream(
    address: std::net::SocketAddr,
    path: &str,
) -> NetworkResult<tokio::net::TcpStream> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut socket = tokio::net::TcpStream::connect(address).await?;
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut buffer = [0_u8; 2048];
        loop {
            let read = socket.read(&mut buffer).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "stream ended before first event",
                ));
            }
            response.extend_from_slice(&buffer[..read]);
            if response
                .windows(b"scope=".len())
                .any(|part| part == b"scope=")
            {
                return Ok(());
            }
        }
    })
    .await??;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    if path == "/scope-sse" {
        assert!(String::from_utf8_lossy(&response).contains("text/event-stream"));
    }
    Ok(socket)
}

async fn scope_event<T>(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<T>,
) -> NetworkResult<T> {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await?
        .ok_or_else(|| "scope event channel closed".into())
}

#[tokio::test]
async fn real_tcp_stream_and_sse_keep_scope_until_disconnect_and_cleanup_once() -> NetworkResult<()>
{
    for path in ["/scope-stream", "/scope-sse"] {
        let (server, probe, mut events) = NetworkScopeServer::start().await?;
        let socket = open_scope_stream(server.address, path).await?;
        let scope = scope_event(&mut events.scopes).await?;
        let resource = scope.resolve::<NetworkScopedResource>().await?;
        assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 0);
        drop(socket);
        assert_eq!(scope_event(&mut events.started).await?, resource.0);
        assert_eq!(
            scope
                .resolve::<NetworkScopedResource>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("scope_closed".to_owned())
        );
        probe.release.add_permits(1);
        assert_eq!(scope_event(&mut events.completed).await?, resource.0);
        server.finish().await?;
        assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 1);
        assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn real_tcp_app_shutdown_waits_for_disconnected_scope_async_cleanup() -> NetworkResult<()> {
    use futures_util::FutureExt as _;
    let (server, probe, mut events) = NetworkScopeServer::start().await?;
    let socket = open_scope_stream(server.address, "/scope-stream").await?;
    let scope = scope_event(&mut events.scopes).await?;
    let id = scope.resolve::<NetworkScopedResource>().await?.0;
    drop(socket);
    assert_eq!(scope_event(&mut events.started).await?, id);
    let app = Arc::clone(&server.app);
    let shutdown = app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5));
    tokio::pin!(shutdown);
    assert!(
        shutdown.as_mut().now_or_never().is_none(),
        "shutdown must wait for the registered cleanup"
    );
    assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 0);
    probe.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), shutdown).await??;
    assert_eq!(scope_event(&mut events.completed).await?, id);
    server.finish().await?;
    assert_eq!(probe.cleanup_starts.load(Ordering::SeqCst), 1);
    assert_eq!(probe.cleanup_finishes.load(Ordering::SeqCst), 1);
    Ok(())
}

async fn request_scope_identity(
    address: std::net::SocketAddr,
    request_id: &str,
) -> NetworkResult<serde_json::Value> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let mut socket = tokio::net::TcpStream::connect(address).await?;
    socket.write_all(format!("GET /scope-identity HTTP/1.1\r\nHost: {address}\r\nx-request-id: {request_id}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut bytes)).await??;
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or("HTTP headers missing")?;
    Ok(serde_json::from_slice(&bytes[split + 4..])?)
}

#[tokio::test]
async fn real_tcp_concurrent_requests_isolate_scope_and_plain_spawn_identity() -> NetworkResult<()>
{
    let (server, _, _) = NetworkScopeServer::start().await?;
    let (left, right) = tokio::join!(
        request_scope_identity(server.address, "scope-network-left"),
        request_scope_identity(server.address, "scope-network-right"),
    );
    let (left, right) = (left?, right?);
    assert_ne!(left["id"], right["id"]);
    assert_eq!(left["request_id"], "scope-network-left");
    assert_eq!(right["request_id"], "scope-network-right");
    for body in [&left, &right] {
        assert_eq!(body["id"], body["again"]);
        assert_eq!(body["plain_inherited"], false);
        assert_ne!(body["id"], body["plain_id"]);
    }
    assert_eq!(left["plain_id"], right["plain_id"]);
    server.finish().await
}

async fn managed_method_probe(
    request: openrust::http::RequestInfo,
) -> openrust::Result<Json<serde_json::Value>> {
    let route = request
        .route()?
        .ok_or_else(|| openrust::Error::custom("test_route_missing", "受管理处理器没有匹配路由"))?;
    Ok(Json(serde_json::json!({
        "name": request.route_name()?, "uri": route.uri(), "methods": route.methods(),
        "fallback": request.route_parameter("fallbackPlaceholder")?,
        "fallback_default": request.route_parameter_or("fallbackPlaceholder", serde_json::json!("fallback"))?,
    })))
}

async fn managed_method_marker(
    info: openrust::http::RequestInfo,
    request: Request,
    next: Next,
) -> openrust::Result<Response> {
    let name = info.route_name()?;
    let mut response = next.run(request).await;
    if let Some(value) = name.and_then(|name| axum::http::HeaderValue::from_str(&name).ok()) {
        response.headers_mut().insert("x-managed-route", value);
    }
    Ok(response)
}

// 捕获 QUERY/自定义方法被标准方法过滤、any 错收 TRACE/CONNECT，以及 HEAD 或路由 middleware 无真实命中。
#[tokio::test]
async fn managed_methods_publish_context_for_query_any_custom_and_chains() -> TestResult {
    use openrust::http::routing;
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(|routes| {
            routes.group(
                RouteGroup::new().middleware(axum::middleware::from_fn(managed_method_marker)),
                |routes| {
                    routes
                        .route("/query", routing::query(managed_method_probe))
                        .name("managed.query")
                        .route("/any", routing::any(managed_method_probe))
                        .name("managed.any")
                        .route(
                            "/custom",
                            routing::on(&["PURGE", "QUERY"], managed_method_probe),
                        )
                        .name("managed.custom")
                        .route(
                            "/chain",
                            routing::get(managed_method_probe).post(managed_method_probe),
                        )
                        .name("managed.chain")
                },
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for (path, name, methods) in [
        ("/query", "managed.query", vec!["QUERY"]),
        ("/custom", "managed.custom", vec!["PURGE", "QUERY"]),
        ("/chain", "managed.chain", vec!["GET", "HEAD", "POST"]),
        (
            "/any",
            "managed.any",
            vec![
                "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "QUERY",
            ],
        ),
    ] {
        for method in &methods {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(*method)
                        .uri(path)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
            assert_eq!(response.headers()["x-managed-route"], name);
            if *method == "HEAD" {
                assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
            } else {
                let json = json_body(response).await;
                assert_eq!(json["data"]["name"], name);
                assert_eq!(json["data"]["uri"], &path[1..]);
                let actual = json["data"]["methods"].as_array().ok_or("缺少方法快照")?;
                assert_eq!(actual.len(), methods.len());
                for expected in &methods {
                    assert!(actual.iter().any(|value| value.as_str() == Some(*expected)));
                }
            }
        }
    }
    for method in ["TRACE", "CONNECT", "PURGE"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/any")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method}"
        );
        assert!(!response.headers().contains_key("x-managed-route"));
    }
    Ok(())
}

async fn route_tenant_collision(
    request: openrust::http::RequestInfo,
) -> openrust::Result<Json<serde_json::Value>> {
    let route = request
        .route()?
        .ok_or_else(|| openrust::Error::custom("test_route_missing", "缺少匹配路由"))?;
    Ok(Json(
        serde_json::json!({"tenant": request.route_parameter("tenant")?, "parameters": route.parameters()}),
    ))
}

// 捕获路由名称模式误用正则、大小写折叠、多星重叠消费或按字节破坏 Unicode 的缺陷。
#[test]
fn route_named_matches_literal_case_sensitive_unicode_and_nonoverlapping_wildcards() {
    use openrust::http::Route;
    for (name, patterns, expected) in [
        (Some("admin.users.show"), vec!["admin.users.show"], true),
        (Some("admin.users.show"), vec!["Admin.users.show"], false),
        (Some("admin.users.show"), vec![""], false),
        (Some("admin.users.show"), vec![], false),
        (
            Some("admin.users.show"),
            vec!["missing", "admin.*.show"],
            true,
        ),
        (Some("admin.users.show"), vec!["admin.**.*"], true),
        (Some("abc"), vec!["ab*bc"], false),
        (Some("abc"), vec!["a*b*c"], true),
        (Some("abc"), vec!["a*c*b"], false),
        (Some("aba"), vec!["aba*aba"], false),
        (Some(""), vec![""], true),
        (Some(""), vec!["*"], true),
        (None, vec!["*", ""], false),
        (Some("文章.查看.甲"), vec!["文章.*.甲"], true),
        (Some("文章.查看.甲"), vec!["文章.*.乙"], false),
        (Some("a.[b]+(c)?^$\\d"), vec!["a.[b]+(c)?^$\\d"], true),
        (Some("a.bbbc"), vec!["a.[b]+(c)?"], false),
        (Some("a.[b]+(c)?"), vec!["a.*(c)?"], true),
    ] {
        let route = Route::new(
            name.map(str::to_owned),
            "/",
            None,
            vec!["GET".into()],
            vec![],
        );
        assert_eq!(
            route.named(&patterns),
            expected,
            "name={name:?}, patterns={patterns:?}"
        );
    }
}

// 捕获公开 resolver setter 不共享 clone、错误/None 被吞掉、默认值伪造未匹配 Route，以及跨请求共享状态。
#[tokio::test]
async fn request_info_route_resolver_supports_trusted_overrides_clones_errors_and_isolation()
-> TestResult {
    use openrust::http::RequestInfo;
    let retained = Arc::new(Mutex::new(Vec::<RequestInfo>::new()));
    let captures = Arc::clone(&retained);
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(TestRoutes::new(move |routes| {
            let captures = Arc::clone(&captures);
            routes
                .get("/resolver/{id}", move |info: RequestInfo| {
                    let captures = Arc::clone(&captures);
                    async move { exercise_trusted_route_resolver(&info, &captures) }
                })
                .name("resolver.original")
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for id in ["first", "second"] {
        let response = router
            .clone()
            .oneshot(Request::get(format!("/resolver/{id}")).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["data"], id);
    }
    let requests = retained
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].route_name()?, Some("trusted.first".into()));
    assert_eq!(requests[1].route_name()?, Some("trusted.second".into()));
    assert_eq!(
        requests[0].route_parameter("id")?,
        Some(serde_json::json!("first"))
    );
    assert_eq!(
        requests[1].route_parameter("id")?,
        Some(serde_json::json!("second"))
    );
    Ok(())
}

#[derive(Clone)]
struct RouteObservation {
    before: Option<openrust::http::Route>,
    before_default: Option<serde_json::Value>,
    after: Option<openrust::http::Route>,
}

type RouteObservations = Arc<Mutex<Vec<RouteObservation>>>;

struct GlobalRouteObserver(RouteObservations);

impl openrust::core::ServiceProvider for GlobalRouteObserver {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, openrust::Result<()>> {
        let observations = Arc::clone(&self.0);
        Box::pin(async move {
            app.http_layer(axum::middleware::from_fn_with_state(
                observations,
                observe_global_route,
            ))?;
            Ok(())
        })
    }
}

async fn observe_global_route(
    axum::extract::State(observations): axum::extract::State<RouteObservations>,
    info: openrust::http::RequestInfo,
    request: Request,
    next: Next,
) -> openrust::Result<Response> {
    let before = info.route()?;
    let before_default = info.route_parameter_or("absent", serde_json::json!("fallback"))?;
    let response = next.run(request).await;
    let after = info.route()?;
    observations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(RouteObservation {
            before,
            before_default,
            after,
        });
    Ok(response)
}

fn observed_route(observations: &RouteObservations) -> Option<openrust::http::Route> {
    let mut observations = observations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        observations.len(),
        1,
        "每次请求只经过一次公开全局 middleware"
    );
    let observation = observations.remove(0);
    assert_eq!(observation.before, None);
    assert_eq!(observation.before_default, None);
    observation.after
}

async fn reject_matched_route(
    info: openrust::http::RequestInfo,
    _request: Request,
    _next: Next,
) -> openrust::Result<Response> {
    assert!(
        info.route()?.is_some(),
        "路由 middleware 执行前必须发布匹配"
    );
    Ok(StatusCode::FORBIDDEN.into_response())
}

// 捕获用状态码清除真实命中、全局前过早发布，或路由短路没有向外层发布上下文的缺陷。
#[tokio::test]
async fn global_route_context_is_empty_before_and_preserves_business_errors_and_short_circuits()
-> TestResult {
    let observations = RouteObservations::default();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let middleware_calls = Arc::new(AtomicUsize::new(0));
    let capture_handler = Arc::clone(&handler_calls);
    let capture_middleware = Arc::clone(&middleware_calls);
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(GlobalRouteObserver(Arc::clone(&observations)))
        .provider(TestRoutes::new(move |routes| {
            global_context_routes(
                routes,
                Arc::clone(&capture_handler),
                Arc::clone(&capture_middleware),
            )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for status in [404, 405, 500] {
        let response = router
            .clone()
            .oneshot(Request::get(format!("/business/{status}")).body(Body::empty())?)
            .await?;
        assert_eq!(response.status().as_u16(), status);
        let route = observed_route(&observations).ok_or("业务错误丢失路由")?;
        assert_eq!(route.name(), Some("business.error"));
        assert_eq!(
            route.parameter("status"),
            Some(&serde_json::json!(status.to_string()))
        );
    }
    let response = router
        .clone()
        .oneshot(Request::get("/blocked").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        observed_route(&observations).ok_or("短路丢失路由")?.name(),
        Some("business.blocked")
    );
    let response = router
        .clone()
        .oneshot(Request::get("/unknown").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(observed_route(&observations), None);
    let response = router
        .clone()
        .oneshot(Request::post("/business/404").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(observed_route(&observations), None);
    // 外置原生 layer 先于框架入口提取 Info，也必须插入并复用同一个请求 slot。
    let external_observations = RouteObservations::default();
    let router = router.layer(axum::middleware::from_fn_with_state(
        Arc::clone(&external_observations),
        observe_global_route,
    ));
    let response = router
        .clone()
        .oneshot(Request::get("/business/404").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let outer =
        observed_route(&external_observations).ok_or("外置 middleware 的快照没有共享实际匹配")?;
    let inner = observed_route(&observations).ok_or("框架全局 middleware 丢失匹配")?;
    assert_eq!(outer, inner);
    assert_eq!(outer.name(), Some("business.error"));
    assert_eq!(outer.parameter("status"), Some(&serde_json::json!("404")));
    assert_invalid_utf8_without_business_side_effects(
        router,
        external_observations,
        observations,
        handler_calls,
        middleware_calls,
    )
    .await?;
    Ok(())
}

async fn mark_get_declaration(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-declaration", axum::http::HeaderValue::from_static("get"));
    response
}

async fn mark_post_declaration(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-declaration",
        axum::http::HeaderValue::from_static("post"),
    );
    response
}

async fn mark_head_declaration(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-declaration",
        axum::http::HeaderValue::from_static("head"),
    );
    response
}

// 捕获同 URI 聚合时跨方法串用 name/middleware、GET 覆盖失效，或显式 HEAD 不服从注册顺序的缺陷。
#[tokio::test]
async fn separate_method_declarations_keep_group_merge_metadata_and_head_registration_order()
-> TestResult {
    for head_first in [true, false] {
        let observations = RouteObservations::default();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(
            FrameworkConfig::default(),
        ));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(GlobalRouteObserver(Arc::clone(&observations)))
            .provider(TestRoutes::new(move |routes| {
                register_separate_method_declarations(routes, head_first)
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        for (method, path, name, marker, methods) in [
            ("GET", "/shared", "methods.get", "get", vec!["GET", "HEAD"]),
            ("POST", "/shared", "methods.post", "post", vec!["POST"]),
            (
                "GET",
                "/overridden",
                "methods.latest",
                "get",
                vec!["GET", "HEAD"],
            ),
            (
                "GET",
                "/head",
                "methods.head-get",
                "get",
                vec!["GET", "HEAD"],
            ),
            (
                "HEAD",
                "/head",
                if head_first {
                    "methods.head-get"
                } else {
                    "methods.head"
                },
                if head_first { "get" } else { "head" },
                if head_first {
                    vec!["GET", "HEAD"]
                } else {
                    vec!["HEAD"]
                },
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{method}{path}");
            assert_eq!(response.headers()["x-declaration"], marker);
            let route = observed_route(&observations).ok_or("缺少独立方法匹配")?;
            assert_eq!(route.name(), Some(name));
            assert_eq!(route.methods(), methods.as_slice());
            if method == "HEAD" {
                assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
            } else {
                assert_eq!(json_body(response).await["data"]["name"], name);
            }
        }
    }
    Ok(())
}

// 捕获自动 OPTIONS 缺少真实快照、custom 污染 Allow、域 fallback 越界及静态资源伪记业务 Route。
#[tokio::test]
async fn options_context_fallback_parameters_and_domain_scopes_follow_actual_dispatch() -> TestResult
{
    for root_fallback in [false, true] {
        let observations = RouteObservations::default();
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("asset.txt"), "static asset")?;
        let mut config = FrameworkConfig::default();
        config.server.public_dir = directory.path().to_owned();
        let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
        let mut builder = openrust::core::ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(GlobalRouteObserver(Arc::clone(&observations)))
            .provider(TestRoutes::new(move |routes| {
                register_options_fallback_routes(routes, root_fallback)
            }));
        let app = builder
            .start(sources)
            .await
            .and_then(openrust::App::from_provider_runtime)?;
        let router = build_router(&app)?;
        assert_automatic_options_context(router.clone(), Arc::clone(&observations), root_fallback)
            .await?;
        assert_similar_prefix_is_outside_domain_fallback(
            router.clone(),
            Arc::clone(&observations),
            root_fallback,
        )
        .await?;
        assert_domain_method_misses_and_merged_metadata(
            router.clone(),
            Arc::clone(&observations),
            root_fallback,
        )
        .await?;
        assert_nonempty_scoped_fallback_parameters(router.clone(), Arc::clone(&observations))
            .await?;
        assert_empty_fallback_parameters(router.clone(), Arc::clone(&observations), root_fallback)
            .await?;
        assert_exact_domain_fallback_parameters(router.clone(), Arc::clone(&observations)).await?;
        assert_root_fallback_and_static_boundaries(
            router.clone(),
            Arc::clone(&observations),
            root_fallback,
        )
        .await?;
    }
    Ok(())
}

// 捕获内部 forward 保留入口候选、域候选 miss 未回滚，或最终参数/domain 泄入下一请求的缺陷。
#[tokio::test]
async fn global_context_tracks_forward_target_and_discards_domain_candidate_misses() -> TestResult {
    let observations = RouteObservations::default();
    let mut config = FrameworkConfig::default();
    config.server.root_target = "forward:/api/target/99".into();
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(config));
    let mut builder = openrust::core::ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(GlobalRouteObserver(Arc::clone(&observations)))
        .provider(TestRoutes::new(|routes| {
            routes
                .get("/api/target/{id}", managed_method_probe)
                .name("forward.target")
                .group(
                    RouteGroup::new().domain("{account}.example.test"),
                    |routes| {
                        routes
                            .post("/api/target/{id}", managed_method_probe)
                            .name("domain.post")
                            .get("/domain/{id}", managed_method_probe)
                            .name("domain.get")
                    },
                )
        }));
    let app = builder
        .start(sources)
        .await
        .and_then(openrust::App::from_provider_runtime)?;
    let router = build_router(&app)?;
    for path in ["/", "/api/target/42"] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, "acme.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let route = observed_route(&observations).ok_or("forward 或候选 miss 后没有最终路由")?;
        assert_eq!(route.name(), Some("forward.target"));
        assert_eq!(route.uri(), "api/target/{id}");
        assert_eq!(route.domain(), None);
        assert_eq!(
            route.parameters(),
            &[(
                "id".to_owned(),
                serde_json::json!(if path == "/" { "99" } else { "42" })
            )]
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::get("/domain/7")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        observed_route(&observations)
            .ok_or("域路由没有命中")?
            .domain(),
        Some("{account}.example.test")
    );
    for (method, path, host, expected) in [
        (
            "GET",
            "/not-matched",
            "acme.example.test",
            StatusCode::NOT_FOUND,
        ),
        ("GET", "/domain/7", "other.invalid", StatusCode::NOT_FOUND),
        (
            "POST",
            "/domain/7",
            "acme.example.test",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), expected);
        assert_eq!(observed_route(&observations), None);
    }
    Ok(())
}

async fn assert_fallback_static_resources(router: axum::Router) -> TestResult {
    let static_asset = router
        .clone()
        .oneshot(Request::get("/asset.txt").body(Body::empty())?)
        .await?;
    assert_eq!(static_asset.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(static_asset.into_body(), usize::MAX).await?,
        "static asset"
    );

    for (path, expected) in [
        ("/api/existing.txt", "api static"),
        ("/ws/socket.txt", "ws static"),
    ] {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(to_bytes(response.into_body(), usize::MAX).await?, expected);
    }

    let static_head = router
        .clone()
        .oneshot(Request::head("/asset.txt").body(Body::empty())?)
        .await?;
    assert_eq!(static_head.status(), StatusCode::OK);
    assert!(
        to_bytes(static_head.into_body(), usize::MAX)
            .await?
            .is_empty()
    );
    Ok(())
}

fn assert_trusted_resolver_failure<T>(result: openrust::Result<T>) -> openrust::Result<()> {
    match result {
        Err(error) => assert_eq!(error.code(), "test_resolver_failed"),
        Ok(_) => {
            return Err(openrust::Error::custom(
                "test_resolver_unexpected_success",
                "resolver 失败未传播",
            ));
        }
    }
    Ok(())
}

fn exercise_trusted_route_resolver(
    info: &openrust::http::RequestInfo,
    captures: &Arc<Mutex<Vec<openrust::http::RequestInfo>>>,
) -> openrust::Result<Json<serde_json::Value>> {
    use openrust::http::{Route, RouteResolver};
    // 每个请求从真实匹配开始，即使上一请求的 clone 仍被测试保留。
    assert_eq!(info.route_name()?, Some("resolver.original".into()));
    let id = info
        .route_parameter("id")?
        .ok_or_else(|| openrust::Error::custom("test_id_missing", "缺少路径参数"))?;
    let original = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("test_route_missing", "缺少真实匹配"))?;
    let original_resolver = info.get_route_resolver()?;
    assert_eq!(original_resolver()?, Some(original.clone()));
    let clone = info.clone();
    let trusted = Route::new(
        Some(format!("trusted.{}", id.as_str().unwrap_or_default())),
        "trusted/{id}",
        Some("trusted.example.test".into()),
        vec!["PURGE".into()],
        vec![
            ("id".into(), id.clone()),
            ("null".into(), serde_json::Value::Null),
        ],
    );
    let custom: RouteResolver = Arc::new({
        let trusted = trusted.clone();
        move || Ok(Some(trusted.clone()))
    });
    info.set_route_resolver(custom)?;
    assert_eq!(clone.route()?, Some(trusted.clone()));
    assert!(clone.route_is(&["missing.*", "trusted.*"])?);
    assert_eq!(clone.get_route_resolver()?()?, Some(trusted.clone()));
    assert_eq!(
        clone.route_parameter_or("missing", serde_json::json!("fallback"))?,
        Some(serde_json::json!("fallback"))
    );
    assert_eq!(
        clone.route_parameter_or("null", serde_json::json!("fallback"))?,
        Some(serde_json::Value::Null)
    );
    assert_eq!(
        clone.route_parameter_or("id", serde_json::json!("fallback"))?,
        Some(id.clone())
    );
    // 构造器与 owned 快照不因后续 setter 被改写。
    assert_eq!(trusted.domain(), Some("trusted.example.test"));
    assert_eq!(trusted.methods(), &["PURGE".to_owned()]);
    assert_eq!(original.name(), Some("resolver.original"));
    clone.set_route_resolver(Arc::new(|| Ok(None)))?;
    assert_eq!(info.route()?, None);
    assert_eq!(info.route_name()?, None);
    assert!(!info.route_is(&["*"])?);
    assert_eq!(info.route_parameter("id")?, None);
    assert_eq!(
        info.route_parameter_or("missing", serde_json::json!("fallback"))?,
        None
    );
    assert_eq!(info.get_route_resolver()?()?, None);
    info.set_route_resolver(Arc::new(|| {
        Err(openrust::Error::custom(
            "test_resolver_failed",
            "可信应用 resolver 失败",
        ))
    }))?;
    assert_trusted_resolver_failure(clone.route())?;
    assert_trusted_resolver_failure(clone.route_name())?;
    assert_trusted_resolver_failure(clone.route_is(&["*"]))?;
    assert_trusted_resolver_failure(
        clone.route_parameter_or("missing", serde_json::json!("fallback")),
    )?;
    assert_trusted_resolver_failure(clone.get_route_resolver()?())?;
    // get 后 set 回原闭包仍读取真实命中，不在内部递归调用当前 resolver。
    clone.set_route_resolver(original_resolver)?;
    assert_eq!(info.route()?, Some(original));
    info.set_route_resolver(Arc::new(move || Ok(Some(trusted.clone()))))?;
    captures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(clone);
    Ok::<_, openrust::Error>(Json(id))
}

fn global_context_routes(
    routes: Routes,
    handler_calls: Arc<AtomicUsize>,
    middleware_calls: Arc<AtomicUsize>,
) -> Routes {
    routes
        .get("/business/{status}", |Path(status): Path<u16>| async move {
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
        })
        .name("business.error")
        .group(
            RouteGroup::new().middleware(axum::middleware::from_fn(
                move |request: Request, next: Next| {
                    middleware_calls.fetch_add(1, Ordering::SeqCst);
                    async move { next.run(request).await }
                },
            )),
            move |routes| {
                routes
                    .get("/item/{id}", move || {
                        let handler_calls = Arc::clone(&handler_calls);
                        async move {
                            handler_calls.fetch_add(1, Ordering::SeqCst);
                            "without Path extractor"
                        }
                    })
                    .name("business.item")
            },
        )
        .group(
            RouteGroup::new().middleware(axum::middleware::from_fn(reject_matched_route)),
            |routes| {
                routes
                    .get("/blocked", managed_method_probe)
                    .name("business.blocked")
            },
        )
}

fn register_separate_method_declarations(routes: Routes, head_first: bool) -> Routes {
    let routes = routes
        .group(
            RouteGroup::new().middleware(axum::middleware::from_fn(mark_get_declaration)),
            |routes| {
                routes
                    .get("/shared", managed_method_probe)
                    .name("methods.get")
            },
        )
        .merge(
            Routes::new()
                .post("/shared", managed_method_probe)
                .name("methods.post")
                .layer(axum::middleware::from_fn(mark_post_declaration)),
        )
        .get("/overridden", first_domain_marker)
        .name("methods.old")
        .group(
            RouteGroup::new().middleware(axum::middleware::from_fn(mark_get_declaration)),
            |routes| {
                routes
                    .get("/overridden", managed_method_probe)
                    .name("methods.latest")
            },
        );
    let head = |routes: Routes| {
        routes.group(
            RouteGroup::new().middleware(axum::middleware::from_fn(mark_head_declaration)),
            |routes| {
                routes
                    .head("/head", managed_method_probe)
                    .name("methods.head")
            },
        )
    };
    let get = |routes: Routes| {
        routes.group(
            RouteGroup::new().middleware(axum::middleware::from_fn(mark_get_declaration)),
            |routes| {
                routes
                    .get("/head", managed_method_probe)
                    .name("methods.head-get")
            },
        )
    };
    if head_first {
        get(head(routes))
    } else {
        head(get(routes))
    }
}

fn register_options_fallback_routes(routes: Routes, root_fallback: bool) -> Routes {
    let routes = routes
        .get("/known/{id}", managed_method_probe)
        .name("options.known")
        .query("/query", managed_method_probe)
        .name("options.query")
        .on("/custom", &["PURGE"], managed_method_probe)
        .name("options.custom")
        .group(RouteGroup::new().prefix("/fallback"), |routes| {
            routes
                .fallback(managed_method_probe)
                .name("prefixed.fallback")
        })
        .group(
            RouteGroup::new()
                .prefix("/short")
                .middleware(axum::middleware::from_fn(reject_matched_route)),
            |routes| routes.fallback(managed_method_probe).name("short.fallback"),
        )
        .group(
            RouteGroup::new()
                .prefix("/tenant")
                .domain("{account}.example.test"),
            |routes| {
                routes
                    .merge(
                        Routes::new()
                            .get("/merged/{id}", managed_method_probe)
                            .name("domain.merged"),
                    )
                    .fallback(managed_method_probe)
            },
        );
    if root_fallback {
        routes.fallback(managed_method_probe).name("root.fallback")
    } else {
        routes
    }
}

fn automatic_options_cases(
    root_fallback: bool,
) -> [(&'static str, &'static str, Option<&'static str>); 7] {
    [
        ("/known/42", "localhost", Some("GET,HEAD")),
        (
            "/query",
            "localhost",
            Some(if root_fallback {
                "GET,HEAD,QUERY"
            } else {
                "QUERY"
            }),
        ),
        (
            "/custom",
            "localhost",
            if root_fallback {
                Some("GET,HEAD")
            } else {
                None
            },
        ),
        (
            "/missing",
            "localhost",
            if root_fallback {
                Some("GET,HEAD")
            } else {
                None
            },
        ),
        ("/tenant/missing", "acme.example.test", Some("GET,HEAD")),
        (
            "/outside",
            "acme.example.test",
            if root_fallback {
                Some("GET,HEAD")
            } else {
                None
            },
        ),
        (
            "/tenant/missing",
            "other.invalid",
            if root_fallback {
                Some("GET,HEAD")
            } else {
                None
            },
        ),
    ]
}

async fn assert_automatic_options_context(
    router: axum::Router,
    observations: RouteObservations,
    root_fallback: bool,
) -> TestResult {
    for (path, host, allow) in automatic_options_cases(root_fallback) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        let route = observed_route(&observations);
        if let Some(allow) = allow {
            assert_eq!(response.status(), StatusCode::OK, "{host}{path}");
            assert_eq!(response.headers()[header::ALLOW], allow);
            assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
            let route = route.ok_or("自动 OPTIONS 没有真实匹配")?;
            assert_eq!(route.name(), None);
            assert_eq!(route.methods(), &["OPTIONS".to_owned()]);
            assert_eq!(route.uri(), path.trim_start_matches('/'));
            assert_eq!(route.parameters(), &[] as &[(String, serde_json::Value)]);
        } else {
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{host}{path}");
            assert!(!response.headers().contains_key(header::ALLOW));
            assert_eq!(route, None);
        }
    }
    Ok(())
}

async fn assert_similar_prefix_is_outside_domain_fallback(
    router: axum::Router,
    observations: RouteObservations,
    root_fallback: bool,
) -> TestResult {
    // 相似前缀不是域 fallback 的 scope；只有显式根 fallback 能提供 alternate。
    for method in ["GET", "OPTIONS", "POST"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/tenantish")
                    .header(header::HOST, "acme.example.test")
                    .body(Body::empty())?,
            )
            .await?;
        let route = observed_route(&observations);
        if !root_fallback {
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{method} /tenantish"
            );
            assert!(!response.headers().contains_key(header::ALLOW));
            assert_eq!(route, None);
        } else if method == "POST" {
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
            assert_eq!(response.headers()[header::ALLOW], "GET, HEAD");
            assert_eq!(route, None);
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            let route = route.ok_or("根 fallback 或合成 OPTIONS 缺少匹配")?;
            assert_eq!(route.domain(), None);
            if method == "OPTIONS" {
                assert_eq!(response.headers()[header::ALLOW], "GET,HEAD");
                assert_eq!(route.name(), None);
                assert_eq!(route.methods(), &["OPTIONS".to_owned()]);
                assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
            } else {
                assert_eq!(route.name(), Some("root.fallback"));
                assert_eq!(route.uri(), "{fallbackPlaceholder}");
                assert_eq!(
                    route.parameter("fallbackPlaceholder"),
                    Some(&serde_json::json!("tenantish"))
                );
            }
        }
    }
    Ok(())
}

async fn assert_domain_method_misses_and_merged_metadata(
    router: axum::Router,
    observations: RouteObservations,
    root_fallback: bool,
) -> TestResult {
    for (host, path) in [
        ("acme.example.test", "/outside"),
        ("other.invalid", "/tenant/missing"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::put(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if root_fallback {
                StatusCode::METHOD_NOT_ALLOWED
            } else {
                StatusCode::NOT_FOUND
            }
        );
        assert_eq!(observed_route(&observations), None);
    }
    let response = router
        .clone()
        .oneshot(
            Request::get("/tenant/merged/42")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let route = observed_route(&observations).ok_or("域 merge 没有路由")?;
    assert_eq!(route.domain(), Some("{account}.example.test"));
    assert_eq!(route.uri(), "tenant/merged/{id}");
    assert_eq!(route.name(), Some("domain.merged"));
    assert_eq!(
        route.parameters(),
        &[
            ("account".to_owned(), serde_json::json!("acme")),
            ("id".to_owned(), serde_json::json!("42"))
        ]
    );
    Ok(())
}

async fn assert_nonempty_scoped_fallback_parameters(
    router: axum::Router,
    observations: RouteObservations,
) -> TestResult {
    for (path, host, name, uri, domain, capture) in [
        (
            "/fallback/missing/a%20b%2Fc?fallbackPlaceholder=query",
            "localhost",
            Some("prefixed.fallback"),
            "fallback/{fallbackPlaceholder}",
            None,
            "missing/a b/c",
        ),
        (
            "/tenant/missing/a%20b%2Fc?fallbackPlaceholder=query",
            "acme.example.test",
            None,
            "tenant/{fallbackPlaceholder}",
            Some("{account}.example.test"),
            "missing/a b/c",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, host)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let route = observed_route(&observations).ok_or("显式 fallback 没有真实匹配")?;
        assert_eq!(route.name(), name);
        assert_eq!(route.uri(), uri);
        assert_eq!(route.domain(), domain);
        assert_eq!(route.methods(), &["GET".to_owned(), "HEAD".to_owned()]);
        assert_eq!(
            route.parameter("fallbackPlaceholder"),
            Some(&serde_json::json!(capture))
        );
        if domain.is_some() {
            assert_eq!(route.parameter("account"), Some(&serde_json::json!("acme")));
        }
    }
    Ok(())
}

async fn assert_empty_fallback_parameters(
    router: axum::Router,
    observations: RouteObservations,
    root_fallback: bool,
) -> TestResult {
    // Laravel binder 过滤空字符串捕获；查询中的同名值不能补入路由参数。
    let mut empty_fallbacks = vec![(
        "/fallback?fallbackPlaceholder=query",
        "prefixed.fallback",
        "fallback/{fallbackPlaceholder}",
    )];
    if root_fallback {
        empty_fallbacks.push((
            "/?fallbackPlaceholder=query",
            "root.fallback",
            "{fallbackPlaceholder}",
        ));
    }
    for (path, name, uri) in empty_fallbacks {
        let response = router
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::HOST, "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let route = observed_route(&observations).ok_or("空捕获 fallback 没有真实匹配")?;
        assert_eq!(route.name(), Some(name));
        assert_eq!(route.uri(), uri);
        assert_eq!(route.parameter("fallbackPlaceholder"), None);
        assert!(
            route
                .parameters()
                .iter()
                .all(|(key, _)| key != "fallbackPlaceholder")
        );
        assert_eq!(
            route.parameter_or("fallbackPlaceholder", serde_json::json!("fallback")),
            serde_json::json!("fallback")
        );
        let body = json_body(response).await;
        assert_eq!(body["data"]["fallback"], serde_json::Value::Null);
        assert_eq!(body["data"]["fallback_default"], "fallback");
    }
    Ok(())
}

async fn assert_exact_domain_fallback_parameters(
    router: axum::Router,
    observations: RouteObservations,
) -> TestResult {
    // 精确域组前缀也命中本组 fallback；空路径捕获过滤后不得从 query 补值。
    let response = router
        .clone()
        .oneshot(
            Request::get("/tenant?fallbackPlaceholder=query")
                .header(header::HOST, "acme.example.test")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let route = observed_route(&observations).ok_or("精确域前缀未命中 fallback")?;
    assert_eq!(route.name(), None);
    assert_eq!(route.uri(), "tenant/{fallbackPlaceholder}");
    assert_eq!(route.domain(), Some("{account}.example.test"));
    assert_eq!(route.methods(), &["GET".to_owned(), "HEAD".to_owned()]);
    assert_eq!(
        route.parameters(),
        &[("account".to_owned(), serde_json::json!("acme"))]
    );
    assert_eq!(route.parameter("fallbackPlaceholder"), None);
    assert_eq!(
        route.parameter_or("fallbackPlaceholder", serde_json::json!("fallback")),
        serde_json::json!("fallback")
    );
    let body = json_body(response).await;
    assert_eq!(body["data"]["fallback"], serde_json::Value::Null);
    assert_eq!(body["data"]["fallback_default"], "fallback");
    Ok(())
}

async fn assert_root_fallback_and_static_boundaries(
    router: axum::Router,
    observations: RouteObservations,
    root_fallback: bool,
) -> TestResult {
    if root_fallback {
        let response = router
            .clone()
            .oneshot(
                Request::get("/missing/a%20b%2Fc?fallbackPlaceholder=query")
                    .header(header::HOST, "localhost")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let route = observed_route(&observations).ok_or("根 fallback 没有匹配")?;
        assert_eq!(route.uri(), "{fallbackPlaceholder}");
        assert_eq!(route.name(), Some("root.fallback"));
        assert_eq!(route.domain(), None);
        assert_eq!(route.methods(), &["GET".to_owned(), "HEAD".to_owned()]);
        assert_eq!(
            route.parameter("fallbackPlaceholder"),
            Some(&serde_json::json!("missing/a b/c"))
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::get("/short/missing")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let route = observed_route(&observations).ok_or("fallback 短路没有保留路由")?;
    assert_eq!(route.name(), Some("short.fallback"));
    assert_eq!(
        route.parameter("fallbackPlaceholder"),
        Some(&serde_json::json!("missing"))
    );
    let response = router
        .clone()
        .oneshot(
            Request::get("/asset.txt")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), usize::MAX).await?,
        "static asset"
    );
    assert_eq!(observed_route(&observations), None);
    // 框架参数解码拒绝非法 UTF-8，不将失败的 fallback 发布为业务匹配。
    let response = router
        .oneshot(
            Request::get("/fallback/%FF")
                .header(header::HOST, "localhost")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(observed_route(&observations), None);
    Ok(())
}

async fn assert_invalid_utf8_without_business_side_effects(
    router: axum::Router,
    external_observations: RouteObservations,
    observations: RouteObservations,
    handler_calls: Arc<AtomicUsize>,
    middleware_calls: Arc<AtomicUsize>,
) -> TestResult {
    // 即使 handler 不提取 Path，也必须在任何业务副作用之前拒绝非法 UTF-8。
    let response = router
        .clone()
        .oneshot(Request::get("/item/%FF").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(observed_route(&external_observations), None);
    assert_eq!(observed_route(&observations), None);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert_eq!(middleware_calls.load(Ordering::SeqCst), 0);
    // 有效请求证明计数器确实接在同一路由的 handler/middleware。
    let response = router
        .oneshot(Request::get("/item/valid").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 1);
    assert_eq!(middleware_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observed_route(&external_observations)
            .ok_or("有效参数没有路由")?
            .name(),
        Some("business.item")
    );
    assert_eq!(
        observed_route(&observations)
            .ok_or("有效参数没有内部路由")?
            .parameter("id"),
        Some(&serde_json::json!("valid"))
    );
    Ok(())
}
