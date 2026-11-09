//! 类型中间件覆盖的解析隔离、恢复和请求快照回归。

use crate::config::{FrameworkConfig, NoConfig};
use crate::foundation::{ApplicationDefinition, BootContext, RegisterContext};
use crate::http::HttpServiceProvider;
use crate::http::middleware::TerminableMiddleware;
use crate::routing::RouteGroup;
use crate::support::{BoxFuture, DefaultProviders, ServiceProvider};
use crate::testing::{TestApp, TestAppBuilder};
use crate::{Error, Result};
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response};
use axum::middleware::Next;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

#[derive(Default)]
struct Counts {
    constructed: [AtomicUsize; 2],
    handled: [AtomicUsize; 2],
    terminated: [AtomicUsize; 2],
    parameterized: AtomicUsize,
    changed: Notify,
}
struct Probe<const INDEX: usize>(Arc<Counts>);
type Target = Probe<0>;
type Other = Probe<1>;

impl<const INDEX: usize> TerminableMiddleware for Probe<INDEX> {
    fn handle<'a>(
        &'a self,
        request: Request<Body>,
        next: Next,
        parameters: &'a [String],
    ) -> BoxFuture<'a, Result<Response<Body>>> {
        Box::pin(async move {
            self.0.handled[INDEX].fetch_add(1, Ordering::SeqCst);
            if INDEX == 0 && !parameters.is_empty() {
                assert!(parameters == ["first"] || parameters == ["second"]);
                self.0.parameterized.fetch_add(1, Ordering::SeqCst);
            }
            Ok(next.run(request).await)
        })
    }

    fn terminate<'a>(
        &'a self,
        _: &'a Request<()>,
        _: &'a Response<()>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.terminated[INDEX].fetch_add(1, Ordering::SeqCst);
            self.0.changed.notify_one();
            Ok(())
        })
    }
}

struct MiddlewareProvider;
impl ServiceProvider for MiddlewareProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.bind::<Target, _>(|resolver| {
                Box::pin(async move {
                    let counts = resolver.resolve::<Counts>().await?;
                    counts.constructed[0].fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(Probe::<0>(counts)))
                })
            })
            .await?;
            app.bind::<Other, _>(|resolver| {
                Box::pin(async move {
                    let counts = resolver.resolve::<Counts>().await?;
                    counts.constructed[1].fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(Probe::<1>(counts)))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.http_middleware_terminable::<Target>()?;
            app.middleware_alias_terminable::<Target>("target-a")?;
            app.middleware_alias_terminable::<Target>("target-b")?;
            app.middleware_alias_terminable::<Other>("other")?;
            app.routes(|routes| {
                routes.group(
                    RouteGroup::new()
                        .middleware_alias("target-a:first")
                        .middleware_alias("target-b:second")
                        .middleware_alias("other"),
                    |routes| routes.get("/probe", || async { "body" }),
                )
            })?;
            Ok(())
        })
    }
}

struct Example;
impl ApplicationDefinition for Example {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![Box::new(HttpServiceProvider), Box::new(MiddlewareProvider)]
    }
}

fn blueprint(counts: &Arc<Counts>) -> TestAppBuilder<Example> {
    let config = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ));
    let mut builder = TestApp::<Example>::builder(config);
    let counts = Arc::clone(counts);
    builder.configure_after_register(move |container| {
        let counts = Arc::clone(&counts);
        Box::pin(async move { container.instance(counts).await })
    });
    builder
}

fn request() -> Result<Request<Body>> {
    Request::builder()
        .uri("/probe")
        .body(Body::empty())
        .map_err(Error::internal)
}

async fn consume(response: Response<Body>) -> Result<()> {
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024)
            .await
            .map_err(Error::internal)?,
        "body"
    );
    Ok(())
}

async fn terminated(counts: &Counts, target: usize, other: usize) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let changed = counts.changed.notified();
            if counts.terminated[0].load(Ordering::SeqCst) >= target
                && counts.terminated[1].load(Ordering::SeqCst) >= other
            {
                break;
            }
            changed.await;
        }
    })
    .await
    .map_err(Error::internal)
}

fn assert_counts(counts: &Counts, target: usize, other: usize) {
    for (index, expected) in [target, other].into_iter().enumerate() {
        assert_eq!(counts.handled[index].load(Ordering::SeqCst), expected);
        assert_eq!(counts.terminated[index].load(Ordering::SeqCst), expected);
        assert_eq!(
            counts.constructed[index].load(Ordering::SeqCst),
            expected * 2
        );
    }
}

#[tokio::test]
async fn type_override_skips_all_aliases_and_global_without_resolving_then_restores() -> Result<()>
{
    let counts = Arc::new(Counts::default());
    let mut builder = blueprint(&counts);
    builder.without_middleware_type::<Target>();
    let mut app = builder.build().await?;
    consume(app.raw_response(request()?).await?).await?;
    terminated(&counts, 0, 1).await?;
    assert_counts(&counts, 0, 1);
    app.with_middleware_type::<Target>();
    consume(app.raw_response(request()?).await?).await?;
    terminated(&counts, 3, 2).await?;
    app.without_middleware_type::<Target>();
    consume(app.raw_response(request()?).await?).await?;
    terminated(&counts, 3, 3).await?;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_counts(&counts, 3, 3);
    assert_eq!(counts.parameterized.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn raw_response_keeps_both_enabled_and_disabled_termination_decisions() -> Result<()> {
    let counts = Arc::new(Counts::default());
    let mut app = blueprint(&counts).build().await?;
    let enabled = app.raw_response(request()?).await?;
    assert_eq!(counts.terminated[0].load(Ordering::SeqCst), 0);
    app.without_middleware_type::<Target>();
    consume(enabled).await?;
    terminated(&counts, 3, 1).await?;
    let disabled = app.raw_response(request()?).await?;
    app.with_middleware_type::<Target>();
    consume(disabled).await?;
    terminated(&counts, 3, 2).await?;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_counts(&counts, 3, 2);
    Ok(())
}

#[tokio::test]
async fn runtime_overrides_are_isolated_and_reset_returns_to_builder_blueprint() -> Result<()> {
    let first = Arc::new(Counts::default());
    let second = Arc::new(Counts::default());
    let mut builder = blueprint(&first);
    builder.without_middleware_type::<Target>();
    let mut app = builder.build().await?;
    let mut independent = blueprint(&second).build().await?;
    app.with_middleware_type::<Target>();
    consume(app.raw_response(request()?).await?).await?;
    terminated(&first, 3, 1).await?;
    app.reset(Instant::now() + Duration::from_secs(5)).await?;
    consume(app.raw_response(request()?).await?).await?;
    consume(independent.raw_response(request()?).await?).await?;
    terminated(&first, 3, 2).await?;
    terminated(&second, 3, 1).await?;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    independent
        .shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_counts(&first, 3, 2);
    assert_counts(&second, 3, 1);
    Ok(())
}

#[tokio::test]
async fn restoring_type_does_not_enable_global_switch_and_builder_restore_is_effective()
-> Result<()> {
    let skipped = Arc::new(Counts::default());
    let mut builder = blueprint(&skipped);
    builder
        .without_middleware()
        .without_middleware_type::<Target>()
        .with_middleware_type::<Target>();
    let mut app = builder.build().await?;
    app.with_middleware_type::<Target>();
    consume(app.raw_response(request()?).await?).await?;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_counts(&skipped, 0, 0);

    let restored = Arc::new(Counts::default());
    let mut builder = blueprint(&restored);
    builder
        .without_middleware_type::<Target>()
        .with_middleware_type::<Target>();
    let mut app = builder.build().await?;
    consume(app.raw_response(request()?).await?).await?;
    terminated(&restored, 3, 1).await?;
    app.shutdown(Instant::now() + Duration::from_secs(5))
        .await?;
    assert_counts(&restored, 3, 1);
    Ok(())
}
