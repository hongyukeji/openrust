//! HTTP 服务器生命周期与 `serve` 命令。

use crate::core::{App, AppState, BoxFuture, Command, Error, Result, shutdown_signal};
use axum::Router;
use std::net::SocketAddr;
use std::{future::Future, time::Duration};
use tokio::net::TcpListener;

use crate::http::plugin::build_router;

/// 绑定 `[server] host:port` 并持续提供服务，直到收到关闭信号。
pub async fn serve(app: &App) -> Result<()> {
    let router = build_router(app)?;
    let addr = app.config().server.bind_addr();
    let listener = TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(app = %app.config().app.name, env = %app.config().app.env, "listening on http://{local}");
    serve_listener(app, listener, router, shutdown_signal()).await
}

async fn serve_listener<F>(
    app: &App,
    listener: TcpListener,
    router: Router,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    serve_listener_inner(
        app.state().clone(),
        Duration::from_secs(app.config().server.shutdown_timeout_secs),
        listener,
        router,
        shutdown,
        true,
    )
    .await
}

/// 在联合 supervisor 下运行 HTTP listener，由外层统一收尾应用后台任务。
#[cfg(feature = "grpc")]
pub(crate) async fn serve_listener_for_supervisor<F>(
    state: AppState,
    timeout: Duration,
    listener: TcpListener,
    router: Router,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    serve_listener_inner(state, timeout, listener, router, shutdown, false).await
}

async fn serve_listener_inner<F>(
    state: AppState,
    timeout: Duration,
    listener: TcpListener,
    router: Router,
    shutdown: F,
    stop_background_tasks: bool,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    // PHP 的 laravel_cloud() 仅识别宿主环境中的精确字符串 `1`；启动时捕获一次。
    let router = if std::env::var("LARAVEL_CLOUD").is_ok_and(|value| value == "1") {
        router.layer(axum::Extension(super::CloudIngress))
    } else {
        router
    };
    let readiness = state.try_get::<super::health::ServerReadiness>()?;
    let _readiness_guard = readiness.begin_listening();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    })
    .into_future();
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => {
            let task_result = if stop_background_tasks {
                let deadline = tokio::time::Instant::now() + timeout;
                state.shutdown_resources(deadline).await
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {
                    task_result?;
                    Err(Error::custom("http_server_stopped", "HTTP server stopped before a shutdown signal"))
                },
                Err(error) => Err(Error::internal(error)),
            }
        }
        () = shutdown => {
            readiness.stop_accepting();
            #[cfg(feature = "realtime")]
            let realtime_shutdown = if state.container().contains::<crate::realtime::Realtime>() {
                match state.resolve::<crate::realtime::Realtime>().await {
                    Ok(realtime) => {
                        realtime.begin_shutdown().await;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            let deadline = tokio::time::Instant::now() + timeout;
            let _ = shutdown_tx.send(());
            let server_result = tokio::time::timeout_at(deadline, &mut server).await;
            let task_result = if stop_background_tasks {
                state.shutdown_resources(deadline).await
            } else {
                Ok(())
            };
            #[cfg(feature = "realtime")]
            realtime_shutdown?;
            if let Ok(result) = server_result {
                result.map_err(Error::internal)?;
                task_result
            } else {
                readiness.cancel_requests();
                Err(Error::custom("http_shutdown_timeout", "HTTP server did not drain before the configured shutdown deadline"))
            }
        }
    };
    crate::core::lifecycle::flush_telemetry().await;
    result
}

/// `serve`：启动 HTTP 服务器。它是每个应用二进制的默认命令。
pub struct ServeCommand;

impl Command for ServeCommand {
    fn name(&self) -> &'static str {
        "serve"
    }

    fn about(&self) -> &'static str {
        "启动 HTTP 服务器"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(serve(app))
    }
}

#[cfg(test)]
mod tests {
    use super::serve_listener;
    use crate::{
        config::FrameworkConfig,
        core::{BootContext, BoxFuture, Error, ProviderBuilder, ServiceProvider},
        http::{HttpServiceProvider, build_router, health::ServerReadiness},
    };
    use axum::{body::Body, http::Request};
    use figment::{Figment, providers::Serialized};
    use std::{io, net::SocketAddr, sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Notify,
    };
    use tower::ServiceExt;

    #[derive(Default)]
    struct Probe {
        started: Notify,
        release: Notify,
    }

    struct BlockingRoutes(Arc<Probe>);

    impl ServiceProvider for BlockingRoutes {
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                let probe = Arc::clone(&self.0);
                app.routes(move |routes| {
                    let probe = Arc::clone(&probe);
                    routes.get("/blocked", move || {
                        let probe = Arc::clone(&probe);
                        async move {
                            probe.started.notify_one();
                            probe.release.notified().await;
                            "finished"
                        }
                    })
                })?;
                Ok(())
            })
        }
    }

    struct ManagedTaskProvider {
        started: Arc<Notify>,
        stopped: Arc<Notify>,
    }

    impl ServiceProvider for ManagedTaskProvider {
        fn boot<'a>(
            &'a self,
            app: &'a mut BootContext<'_>,
        ) -> BoxFuture<'a, crate::core::Result<()>> {
            Box::pin(async move {
                let started = Arc::clone(&self.started);
                let stopped = Arc::clone(&self.stopped);
                app.resources().spawn(move |cancellation| async move {
                    started.notify_one();
                    cancellation.cancelled().await;
                    stopped.notify_one();
                })
            })
        }
    }

    async fn app(shutdown_timeout_secs: u64, probe: Arc<Probe>) -> crate::core::Result<crate::App> {
        app_with_task(shutdown_timeout_secs, probe, None).await
    }

    async fn app_with_task(
        shutdown_timeout_secs: u64,
        probe: Arc<Probe>,
        task: Option<(Arc<Notify>, Arc<Notify>)>,
    ) -> crate::core::Result<crate::App> {
        let mut config = FrameworkConfig::default();
        config.server.shutdown_timeout_secs = shutdown_timeout_secs;
        config.api.http_status = crate::config::HttpStatusMode::Semantic;
        let mut builder = ProviderBuilder::new();
        builder
            .provider(HttpServiceProvider)
            .provider(BlockingRoutes(probe));
        if let Some((started, stopped)) = task {
            builder.provider(ManagedTaskProvider { started, stopped });
        }
        crate::core::App::from_provider_runtime(
            builder
                .start(Figment::from(Serialized::defaults(config)))
                .await?,
        )
    }

    async fn send_request(address: SocketAddr, path: &str) -> io::Result<TcpStream> {
        let mut stream = TcpStream::connect(address).await?;
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await?;
        Ok(stream)
    }

    async fn read_response(address: SocketAddr, path: &str) -> io::Result<Vec<u8>> {
        let mut stream = send_request(address, path).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(response)
    }

    #[tokio::test]
    async fn readiness_changes_after_bind_and_before_graceful_drain() -> crate::core::Result<()> {
        let probe = Arc::new(Probe::default());
        let task_started = Arc::new(Notify::new());
        let task_stopped = Arc::new(Notify::new());
        let app = app_with_task(
            2,
            Arc::clone(&probe),
            Some((Arc::clone(&task_started), Arc::clone(&task_stopped))),
        )
        .await?;
        task_started.notified().await;
        let readiness = app.state().resolve::<ServerReadiness>().await?;
        let router = build_router(&app)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(Error::internal)?;
        let address = listener.local_addr().map_err(Error::internal)?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let serve = tokio::spawn(async move {
            serve_listener(&app, listener, router, async {
                let _ = shutdown_rx.await;
            })
            .await
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            while !readiness.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        let ready_response = read_response(address, "/ready")
            .await
            .map_err(Error::internal)?;
        assert!(String::from_utf8_lossy(&ready_response).starts_with("HTTP/1.1 200"));

        let mut blocked = send_request(address, "/blocked")
            .await
            .map_err(Error::internal)?;
        tokio::time::timeout(Duration::from_secs(2), probe.started.notified())
            .await
            .map_err(Error::internal)?;
        shutdown_tx.send(()).map_err(|()| {
            crate::core::Error::custom(
                "test_shutdown_signal_lost",
                "shutdown signal receiver closed",
            )
        })?;
        tokio::time::timeout(Duration::from_secs(1), async {
            while readiness.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        assert!(!readiness.is_ready());
        let next_request = tokio::time::timeout(
            Duration::from_millis(100),
            read_response(address, "/health"),
        )
        .await;
        assert!(
            next_request.is_err() || next_request.is_ok_and(|result| result.is_err()),
            "server must stop accepting new requests during drain"
        );
        probe.release.notify_one();
        let mut response = Vec::new();
        blocked
            .read_to_end(&mut response)
            .await
            .map_err(Error::internal)?;
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
        tokio::time::timeout(Duration::from_secs(2), serve)
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)??;
        task_stopped.notified().await;
        assert!(!readiness.is_ready());

        Ok(())
    }

    #[tokio::test]
    async fn non_cooperative_request_is_aborted_at_shutdown_deadline() -> crate::core::Result<()> {
        let probe = Arc::new(Probe::default());
        let app = app(1, Arc::clone(&probe)).await?;
        let readiness = app.state().resolve::<ServerReadiness>().await?;
        let router = build_router(&app)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(Error::internal)?;
        let address = listener.local_addr().map_err(Error::internal)?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let serve = tokio::spawn(async move {
            serve_listener(&app, listener, router, async {
                let _ = shutdown_rx.await;
            })
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !readiness.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(Error::internal)?;
        let mut blocked = send_request(address, "/blocked")
            .await
            .map_err(Error::internal)?;
        tokio::time::timeout(Duration::from_secs(2), probe.started.notified())
            .await
            .map_err(Error::internal)?;
        shutdown_tx.send(()).map_err(|()| {
            crate::core::Error::custom(
                "test_shutdown_signal_lost",
                "shutdown signal receiver closed",
            )
        })?;
        let result = tokio::time::timeout(Duration::from_secs(3), serve)
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)?;
        let Err(error) = result else {
            return Err(Error::custom(
                "test_expected_shutdown_timeout",
                "non-cooperative request did not exceed the drain deadline",
            ));
        };
        assert_eq!(error.code(), "http_shutdown_timeout");
        assert!(!readiness.is_ready());
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), blocked.read_to_end(&mut response))
            .await
            .map_err(Error::internal)?
            .map_err(Error::internal)?;
        assert!(
            String::from_utf8_lossy(&response).starts_with("HTTP/1.1 503"),
            "drained request response: {}",
            String::from_utf8_lossy(&response)
        );
        Ok(())
    }

    #[tokio::test]
    async fn bind_failure_never_marks_server_ready() -> crate::core::Result<()> {
        let occupied = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(Error::internal)?;
        let address = occupied.local_addr().map_err(Error::internal)?;
        let mut config = FrameworkConfig::default();
        config.server.host = address.ip().to_string();
        config.server.port = address.port();
        let mut builder = ProviderBuilder::new();
        builder.provider(HttpServiceProvider);
        let app = crate::core::App::from_provider_runtime(
            builder
                .start(Figment::from(Serialized::defaults(config)))
                .await?,
        )?;
        assert!(!app.state().resolve::<ServerReadiness>().await?.is_ready());
        let response = build_router(&app)?
            .oneshot(
                Request::get("/ready")
                    .body(Body::empty())
                    .map_err(Error::internal)?,
            )
            .await
            .map_err(Error::internal)?;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(super::serve(&app).await.is_err());
        assert!(!app.state().resolve::<ServerReadiness>().await?.is_ready());
        Ok(())
    }
}
