use super::*;
use crate::container::ParameterValue;
use crate::events::EventsServiceProvider;
use crate::foundation::events::Terminating;
use crate::foundation::{ProviderBuilder, RegisterContext};
use crate::routing::{Route, RouteGroup};
use crate::support::{BoxFuture, ServiceProvider};
use figment::{Figment, providers::Serialized};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
struct Mark {
    phase: &'static str,
    kind: u8,
    instance: usize,
}

type Sender = mpsc::UnboundedSender<Mark>;

fn record(sender: &Sender, phase: &'static str, kind: u8, instance: usize) -> Result<()> {
    sender
        .send(Mark {
            phase,
            kind,
            instance,
        })
        .map_err(Error::internal)
}

struct Probe<const N: u8>(Sender, usize);

impl<const N: u8> TerminableMiddleware for Probe<N> {
    fn handle<'a>(
        &'a self,
        request: Request,
        next: Next,
        args: &'a [String],
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            record(&self.0, "handle", N, self.1)?;
            if N == 1 {
                let expected = if request.uri().path().starts_with("/short") {
                    &["short"][..]
                } else {
                    &["alpha", "beta"][..]
                };
                assert_eq!(args, expected);
                let attributes = request
                    .extensions()
                    .get::<RequestAttributes>()
                    .ok_or_else(|| Error::custom("test_attributes_missing", "请求属性不可用"))?;
                attributes.insert("audit", ParameterValue::new(23_u8))?;
                if args == ["short"] {
                    return Ok(axum::response::IntoResponse::into_response((
                        StatusCode::FORBIDDEN,
                        "short",
                    )));
                }
            } else {
                assert_eq!(args, &[] as &[String]);
            }
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert("x-termination-probe", http::HeaderValue::from_static("yes"));
            Ok(response)
        })
    }

    fn terminate<'a>(
        &'a self,
        request: &'a http::Request<()>,
        response: &'a http::Response<()>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            assert!(http_entry_scope().is_some());
            assert_eq!(
                response
                    .headers()
                    .get("x-termination-probe")
                    .and_then(|value| value.to_str().ok()),
                Some("yes")
            );
            let route = request.extensions().get::<Route>();
            let expected = match request.uri().path() {
                "/missing" => {
                    assert!(route.is_none());
                    StatusCode::NOT_FOUND
                }
                path => {
                    assert_eq!(
                        route.and_then(|route| route.parameter("item")),
                        Some(&serde_json::json!("42"))
                    );
                    let attributes =
                        request
                            .extensions()
                            .get::<RequestAttributes>()
                            .ok_or_else(|| {
                                Error::custom("test_attributes_missing", "请求属性不可用")
                            })?;
                    let value = attributes
                        .get("audit")?
                        .ok_or_else(|| Error::custom("test_audit_missing", "终止阶段未保留属性"))?;
                    assert_eq!(*value.downcast::<u8>()?, 23);
                    if path.starts_with("/short") {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::OK
                    }
                }
            };
            assert_eq!(response.status(), expected);
            record(&self.0, "terminate", N, self.1)?;
            if N == 1 && request.uri().path().starts_with("/failed") {
                return Err(Error::custom("test_termination_failed", "终止中间件失败"));
            }
            Ok(())
        })
    }
}

struct Routes(Sender, Arc<AtomicUsize>);

impl Routes {
    fn factory<const N: u8>(
        &self,
    ) -> impl Fn(Resolver) -> BoxFuture<'static, Result<Arc<Probe<N>>>> + Send + Sync + 'static
    {
        let sender = self.0.clone();
        let next = Arc::clone(&self.1);
        move |_| {
            let instance = Arc::new(Probe(sender.clone(), next.fetch_add(1, Ordering::Relaxed)));
            Box::pin(async move { Ok(instance) })
        }
    }
}

impl ServiceProvider for Routes {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.bind(self.factory::<0>()).await?;
            app.bind(self.factory::<1>()).await?;
            app.singleton(self.factory::<2>()).await?;
            app.bind(self.factory::<3>()).await
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.http_middleware_terminable::<Probe<0>>()?;
            app.http_middleware_terminable::<Probe<3>>()?;
            app.http_middleware_terminable::<Probe<3>>()?;
            app.middleware_alias_terminable::<Probe<1>>("probe-first")?;
            app.middleware_alias_terminable::<Probe<2>>("probe-last")?;
            app.middleware_group(
                "probe-group",
                [
                    "probe-last",
                    "probe-first:alpha,beta",
                    "probe-first:alpha,beta",
                ],
            )?;
            app.middleware_priority(["probe-first", "probe-last"])?;
            app.routes(|routes| {
                routes
                    .group(
                        RouteGroup::new().middleware_group("probe-group"),
                        |routes| {
                            routes
                                .get("/normal/{item}", || async { "normal" })
                                .get("/failed/{item}", || async { "failed" })
                        },
                    )
                    .group(
                        RouteGroup::new()
                            .middleware_alias("probe-last")
                            .middleware_alias("probe-first:short"),
                        |routes| routes.get("/short/{item}", || async { StatusCode::IM_A_TEAPOT }),
                    )
            })?;
            let sender = self.0.clone();
            app.terminating(move |_| {
                let result = record(&sender, "app", 0, 0);
                Box::pin(async { result })
            });
            Ok(())
        })
    }
}

type Trace = &'static [(&'static str, u8)];

const CASES: &[(&str, &str, Trace)] = &[
    (
        "/normal/42",
        "200",
        &[
            ("handle", 3),
            ("handle", 0),
            ("handle", 1),
            ("handle", 2),
            ("event", 0),
            ("terminate", 1),
            ("terminate", 2),
            ("terminate", 3),
            ("terminate", 0),
            ("app", 0),
        ],
    ),
    (
        "/short/42",
        "403",
        &[
            ("handle", 3),
            ("handle", 0),
            ("handle", 1),
            ("event", 0),
            ("terminate", 1),
            ("terminate", 2),
            ("terminate", 3),
            ("terminate", 0),
            ("app", 0),
        ],
    ),
    (
        "/missing",
        "404",
        &[
            ("handle", 3),
            ("handle", 0),
            ("event", 0),
            ("terminate", 3),
            ("terminate", 0),
            ("app", 0),
        ],
    ),
    (
        "/failed/42",
        "200",
        &[
            ("handle", 3),
            ("handle", 0),
            ("handle", 1),
            ("handle", 2),
            ("event", 0),
            ("terminate", 1),
        ],
    ),
];

#[tokio::test]
async fn terminable_middleware_uses_the_selected_route_and_container_lifetimes() -> Result<()> {
    use crate::config::{FrameworkConfig, HttpStatusMode};

    let (sender, mut records) = mpsc::unbounded_channel();
    let mut events = EventsServiceProvider::new();
    let event = sender.clone();
    events.listen::<Terminating, _>(move |_, _| {
        let result = record(&event, "event", 0, 0);
        Box::pin(async { result })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(HttpServiceProvider)
        .provider(events)
        .provider(Routes(sender, Arc::default()));
    let mut config = FrameworkConfig::default();
    config.api.http_status = HttpStatusMode::Semantic;
    let app = Application::from_provider_runtime(
        builder
            .start(Figment::from(Serialized::defaults(config)))
            .await?,
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(Error::internal)?;
    let address = listener.local_addr().map_err(Error::internal)?;
    let router = build_router(&app)?;
    let (stop, shutdown) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await
    });
    for &(path, status, expected) in CASES {
        let response = super::http_termination_tests::read_response(address, "GET", path).await?;
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{response}"
        );
        let mut instances = [None; 4];
        for &(phase, kind) in expected {
            let mark = tokio::time::timeout(Duration::from_secs(2), records.recv())
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| Error::custom("test_record_missing", "缺少中间件执行记录"))?;
            assert_eq!((mark.phase, mark.kind), (phase, kind));
            if phase == "handle" {
                instances[usize::from(kind)] = Some(mark.instance);
            } else if phase == "terminate"
                && let Some(handled) = instances[usize::from(kind)]
            {
                assert_eq!(mark.instance == handled, kind == 2);
            }
        }
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(30), records.recv())
            .await
            .is_err()
    );
    stop.send(())
        .map_err(|()| Error::custom("test_listener_stopped", "测试监听已关闭"))?;
    server
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
        .await
}
