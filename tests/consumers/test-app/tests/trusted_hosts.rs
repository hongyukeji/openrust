//! 独立应用仅经公开配置和 BootContext 验证 TrustHosts 请求接线。

use figment::{Figment, providers::Serialized};
use openrust::config::{
    Environment, FrameworkConfig, HttpStatusMode, NoConfig, TrustedHostsConfig,
};
use openrust::core::Result;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::http::{
    HttpServiceProvider, RequestInfo,
    axum::{
        Json,
        body::Body,
        extract::connect_info::ConnectInfo,
        http::{Request, StatusCode},
    },
};
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::testing::TestApp;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct HostRoutes;
impl ServiceProvider for HostRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.routes(|routes| {
                routes
                    .get("/host", |info: RequestInfo| async move {
                        Ok::<_, openrust::Error>(Json(serde_json::json!({"host": info.host()?})))
                    })
                    .get("/port", |info: RequestInfo| async move {
                        Ok::<_, openrust::Error>(Json(serde_json::json!({"port": info.port()?})))
                    })
                    .get("/plain", || async { "ok" })
            })?;
            Ok(())
        })
    }
}
struct StaticApplication;
impl ApplicationDefinition for StaticApplication {
    type Config = NoConfig;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(HostRoutes)]
    }
}
fn source(env: Environment, hosts: Option<TrustedHostsConfig>) -> Figment {
    let mut config = FrameworkConfig::default();
    config.app.env = env;
    config.app.url = "http://example.test:8080".into();
    config.api.http_status = HttpStatusMode::Semantic;
    config.server.trusted_hosts = hosts;
    config.server.trusted_proxy_ranges = Some(vec!["REMOTE_ADDR".into()]);
    Figment::from(Serialized::defaults(config))
}
fn request(
    host: &str,
    forwarded: Option<&str>,
) -> std::result::Result<Request<Body>, Box<dyn std::error::Error>> {
    let mut builder = Request::get("/host").header("host", host);
    if let Some(forwarded) = forwarded {
        builder = builder
            .header("x-forwarded-host", forwarded)
            .extension(ConnectInfo("10.0.0.1:42000".parse::<SocketAddr>()?));
    }
    Ok(builder.body(Body::empty())?)
}
async fn close<A: ApplicationDefinition>(app: &mut TestApp<A>) -> Result<()> {
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

#[tokio::test]
async fn defaults_explicit_empty_and_exact_environment()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut disabled = TestApp::<StaticApplication>::builder(source(Environment::Production, None))
        .build()
        .await?;
    disabled
        .request(request("evil.test", None)?)
        .await?
        .assert_status(StatusCode::OK)?;
    close(&mut disabled).await?;

    let mut production = TestApp::<StaticApplication>::builder(source(
        Environment::Production,
        Some(TrustedHostsConfig::default()),
    ))
    .build()
    .await?;
    production
        .request(request("deep.api.example.test:8080", None)?)
        .await?
        .assert_status(StatusCode::OK)?;
    production
        .request(request("evil.test", None)?)
        .await?
        .assert_status(StatusCode::BAD_REQUEST)?;
    close(&mut production).await?;

    let mut empty = TestApp::<StaticApplication>::builder(source(
        Environment::Production,
        Some(TrustedHostsConfig {
            at: Some(vec![]),
            subdomains: false,
        }),
    ))
    .build()
    .await?;
    empty
        .request(request("evil.test", None)?)
        .await?
        .assert_status(StatusCode::OK)?;
    close(&mut empty).await?;

    for env in [Environment::Local, Environment::Testing] {
        let mut app =
            TestApp::<StaticApplication>::builder(source(env, Some(TrustedHostsConfig::default())))
                .build()
                .await?;
        app.request(request("evil.test", None)?)
            .await?
            .assert_status(StatusCode::OK)?;
        close(&mut app).await?;
    }
    for env in [
        Environment::Development,
        Environment::Dev,
        Environment::Test,
    ] {
        let mut app =
            TestApp::<StaticApplication>::builder(source(env, Some(TrustedHostsConfig::default())))
                .build()
                .await?;
        app.request(request("evil.test", None)?)
            .await?
            .assert_status(StatusCode::BAD_REQUEST)?;
        close(&mut app).await?;
    }
    Ok(())
}

#[tokio::test]
async fn effective_forwarded_host_uses_allowlist_after_trusted_proxy()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let hosts = TrustedHostsConfig {
        at: Some(vec![r"^public\.test$".into()]),
        subdomains: false,
    };
    let sources = source(Environment::Production, Some(hosts)).merge(Serialized::defaults(
        serde_json::json!({
            "server": {"trusted_proxy_ranges": ["10.0.0.1/32"]}
        }),
    ));
    let mut app = TestApp::<StaticApplication>::builder(sources)
        .build()
        .await?;
    let response = app
        .request(request("backend.internal:8080", Some("public.test"))?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let json: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(json["host"], "public.test");
    app.request(request("backend.internal", Some("attacker.test"))?)
        .await?
        .assert_status(StatusCode::BAD_REQUEST)?;
    app.request(
        Request::get("/plain")
            .header("host", "backend.internal")
            .header("x-forwarded-host", "attacker.test")
            .extension(ConnectInfo("10.0.0.1:42000".parse::<SocketAddr>()?))
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::BAD_REQUEST)?;
    app.request(
        Request::get("/plain")
            .header("host", "backend.internal")
            .header("x-forwarded-host", "public.test")
            .extension(ConnectInfo("198.51.100.1:42000".parse::<SocketAddr>()?))
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::BAD_REQUEST)?;
    app.request(
        Request::get("/port")
            .header("host", "public.test:8080")
            .header("x-forwarded-host", "attacker.test")
            .extension(ConnectInfo("198.51.100.1:42000".parse::<SocketAddr>()?))
            .body(Body::empty())?,
    )
    .await?
    .assert_status(StatusCode::OK)?;
    close(&mut app).await?;
    Ok(())
}

#[tokio::test]
async fn invalid_pattern_is_evaluated_on_request()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let hosts = TrustedHostsConfig {
        at: Some(vec!["[".into()]),
        subdomains: false,
    };
    let mut app =
        TestApp::<StaticApplication>::builder(source(Environment::Production, Some(hosts)))
            .build()
            .await?;
    app.request(request("example.test", None)?)
        .await?
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    close(&mut app).await?;
    Ok(())
}

struct PolicyCounter(AtomicUsize);
struct DynamicPolicy;
impl ServiceProvider for DynamicPolicy {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(PolicyCounter(AtomicUsize::new(0))))
                .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.trust_hosts_with(false, |state| {
                Box::pin(async move {
                    let counter = state.resolve::<PolicyCounter>().await?;
                    let index = counter.0.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![if index % 2 == 0 {
                        r"^first\.test$".into()
                    } else {
                        r"^second\.test$".into()
                    }])
                })
            })?;
            app.routes(|routes| {
                routes.get("/host", |info: RequestInfo| async move {
                    Ok::<_, openrust::Error>(Json(serde_json::json!({"host": info.host()?})))
                })
            })?;
            Ok(())
        })
    }
}
struct DynamicApplication;
impl ApplicationDefinition for DynamicApplication {
    type Config = NoConfig;

    fn default_providers() -> openrust::support::DefaultProviders {
        openrust::support::DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(DynamicPolicy)]
    }
}
#[tokio::test]
async fn callback_runs_once_per_request_and_isolated_per_app()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut first = TestApp::<DynamicApplication>::builder(source(Environment::Production, None))
        .build()
        .await?;
    let mut second = TestApp::<DynamicApplication>::builder(source(Environment::Production, None))
        .build()
        .await?;
    for host in ["first.test", "second.test"] {
        first
            .request(request(host, None)?)
            .await?
            .assert_status(StatusCode::OK)?;
    }
    let first_count = first.app()?.state().resolve::<PolicyCounter>().await?;
    let second_count = second.app()?.state().resolve::<PolicyCounter>().await?;
    assert_eq!(first_count.0.load(Ordering::SeqCst), 2);
    assert_eq!(second_count.0.load(Ordering::SeqCst), 0);
    second
        .request(request("first.test", None)?)
        .await?
        .assert_status(StatusCode::OK)?;
    assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
    let mut local = TestApp::<DynamicApplication>::builder(source(Environment::Local, None))
        .build()
        .await?;
    local
        .request(request("other.test", None)?)
        .await?
        .assert_status(StatusCode::OK)?;
    assert_eq!(
        local
            .app()?
            .state()
            .resolve::<PolicyCounter>()
            .await?
            .0
            .load(Ordering::SeqCst),
        0
    );
    close(&mut local).await?;
    close(&mut first).await?;
    close(&mut second).await?;
    Ok(())
}
