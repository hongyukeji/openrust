//! 显式组合多个网络 listener 的应用运行入口。

use std::{future::Future, time::Duration};

use crate::{
    App, BoxFuture, Command, Error, Result,
    core::{Readiness, shutdown_signal},
    grpc::GrpcServer,
    http::{build_router, serve::serve_listener_for_supervisor},
};
use tokio::{
    net::TcpListener,
    task::{JoinError, JoinSet},
    time::{Instant, timeout_at},
};

/// 为已注册 HTTP 与 gRPC 能力贡献联合运行命令。
pub struct HttpGrpcServiceProvider;
impl crate::core::ServiceProvider for HttpGrpcServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut crate::core::BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<crate::routes::HttpPluginInstalled>()
                || !app.is_bound::<crate::grpc::GrpcPluginInstalled>()
            {
                return Err(Error::custom(
                    "http_grpc_dependency_missing",
                    "联合运行需要 HTTP 与 gRPC 服务",
                ));
            }
            let registry = app.resolve::<crate::grpc::GrpcServiceRegistry>().await?;
            app.validate_at_finalize(move || {
                Box::pin(async move { registry.validate_contributions(true, false) })
            });
            app.command(ServeAllCommand);
            Ok(())
        })
    }
}

/// 同时运行应用已注册的 HTTP 与 gRPC 服务。
///
/// 两个 listener 都成功绑定后才会启动服务任务。收到进程信号时同时发起优雅停机；
/// 任一服务意外退出时会停止另一个服务。停机超时会取消请求、终止并 join 剩余任务。
pub async fn serve_http_and_grpc(app: &App) -> Result<()> {
    serve_http_and_grpc_until(app, shutdown_signal()).await
}

/// 以调用方提供的退出 future 同时运行 HTTP 与 gRPC，供嵌入式宿主控制生命周期。
pub async fn serve_http_and_grpc_until<F>(app: &App, shutdown: F) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (router, grpc_server) = prepare_http_and_grpc(app).await?;
    let http_listener = TcpListener::bind(app.config().server.bind_addr()).await?;
    let grpc_listener = TcpListener::bind(grpc_server.listen_addr()).await?;
    supervise_http_and_grpc(
        app,
        router,
        grpc_server,
        http_listener,
        grpc_listener,
        shutdown,
    )
    .await
}

/// 使用调用方已绑定的 HTTP 与 gRPC listener 运行同一联合服务生命周期。
///
/// 可使用端口 `0` 绑定后读取两个 listener 的实际地址；两个入口各占一个独立的
/// TCP 监听地址。`server.bind_addr` 与 `grpc.listen_addr` 不用于绑定或校验传入的
/// listener，实际监听范围仅由其 `local_addr` 决定；TLS 等其他安全与生命周期配置
/// 仍照常生效。此入口消费两个 listener，保留正常的 Provider、路由、就绪、共同
/// 停机与资源清理检查。若准备失败，两个 listener 随调用返回一并释放。普通配置绑定仍使用
/// [`serve_http_and_grpc_until`]。
pub async fn serve_http_and_grpc_with_listeners_until<F>(
    app: &App,
    http_listener: TcpListener,
    grpc_listener: TcpListener,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let (router, grpc_server) = prepare_http_and_grpc(app).await?;
    supervise_http_and_grpc(
        app,
        router,
        grpc_server,
        http_listener,
        grpc_listener,
        shutdown,
    )
    .await
}

async fn prepare_http_and_grpc(app: &App) -> Result<(axum::Router, GrpcServer)> {
    if !app
        .state()
        .container()
        .contains::<crate::routes::HttpPluginInstalled>()
    {
        return Err(Error::custom(
            "http_service_missing",
            "联合运行需要 HTTP 服务",
        ));
    }
    let router = build_router(app)?;
    let grpc_server = GrpcServer::from_app(app).await?;
    Ok((router, grpc_server))
}

async fn supervise_http_and_grpc<F>(
    app: &App,
    router: axum::Router,
    grpc_server: GrpcServer,
    http_listener: TcpListener,
    grpc_listener: TcpListener,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let http_address = http_listener.local_addr()?;
    let grpc_address = grpc_listener.local_addr()?;
    let readiness = app.state().resolve::<Readiness>().await?;
    readiness.expect_listeners(2)?;
    let shutdown_timeout = Duration::from_secs(app.config().server.shutdown_timeout_secs);
    let app_state = app.state().clone();
    let background_state = app.state().clone();

    tracing::info!(%http_address, %grpc_address, "HTTP and gRPC listeners bound");

    let (http_shutdown_tx, http_shutdown_rx) = tokio::sync::oneshot::channel();
    let (grpc_shutdown_tx, grpc_shutdown_rx) = tokio::sync::oneshot::channel();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        (
            "http",
            serve_listener_for_supervisor(
                app_state,
                shutdown_timeout,
                http_listener,
                router,
                async move {
                    let _ = http_shutdown_rx.await;
                },
            )
            .await,
        )
    });
    tasks.spawn(async move {
        (
            "grpc",
            grpc_server
                .serve(grpc_listener, async move {
                    let _ = grpc_shutdown_rx.await;
                })
                .await,
        )
    });

    let first_exit =
        stop_on_signal_or_listener_exit(&mut tasks, shutdown, http_shutdown_tx, grpc_shutdown_tx)
            .await;
    readiness.stop_accepting();

    let mut failure = first_exit
        .map_or(Ok(()), |joined| completion_error(joined, true))
        .err();
    let deadline = Instant::now() + shutdown_timeout;
    let drain = timeout_at(deadline, async {
        while let Some(joined) = tasks.join_next().await {
            if let Err(error) = completion_error(Some(joined), false)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        background_state
            .shutdown_resources(deadline)
            .await
            .map_err(Some)
    })
    .await;

    if drain.is_err() {
        readiness.cancel_requests();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        let _ = background_state.shutdown_resources(deadline).await;
        crate::core::lifecycle::flush_telemetry().await;
        return Err(Error::custom(
            "http_grpc_shutdown_timeout",
            "HTTP 与 gRPC 服务未能在共同停机期限内结束",
        ));
    }
    if let Ok(Err(Some(error))) = drain
        && failure.is_none()
    {
        failure = Some(error);
    }
    crate::core::lifecycle::flush_telemetry().await;
    failure.map_or(Ok(()), Err)
}

async fn stop_on_signal_or_listener_exit<F>(
    tasks: &mut JoinSet<(&'static str, Result<()>)>,
    shutdown: F,
    http_shutdown: tokio::sync::oneshot::Sender<()>,
    grpc_shutdown: tokio::sync::oneshot::Sender<()>,
) -> Option<Option<std::result::Result<(&'static str, Result<()>), JoinError>>>
where
    F: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    let first_exit = tokio::select! {
        () = shutdown => None,
        joined = tasks.join_next() => Some(joined),
    };
    let _ = http_shutdown.send(());
    let _ = grpc_shutdown.send(());
    first_exit
}

fn completion_error(
    joined: Option<std::result::Result<(&'static str, Result<()>), JoinError>>,
    unexpected: bool,
) -> Result<()> {
    let Some(joined) = joined else {
        return Err(Error::custom(
            "http_grpc_server_set_empty",
            "HTTP 与 gRPC 服务任务意外结束",
        ));
    };
    let (name, result) = joined.map_err(Error::internal)?;
    match result {
        Err(error) => Err(error),
        Ok(()) if unexpected => Err(Error::custom(
            "http_grpc_server_stopped",
            format!("{name} 服务在退出信号前停止"),
        )),
        Ok(()) => Ok(()),
    }
}

struct ServeAllCommand;

impl Command for ServeAllCommand {
    fn name(&self) -> &'static str {
        "serve all"
    }

    fn about(&self) -> &'static str {
        "同时启动 HTTP 与 gRPC 服务"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(serve_http_and_grpc(app))
    }
}

#[cfg(test)]
mod tests {
    use super::{completion_error, stop_on_signal_or_listener_exit};
    use crate::Error;
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn early_clean_listener_exit_is_reported_as_failure() {
        let mut tasks = JoinSet::new();
        tasks.spawn(async { ("http", Ok::<(), Error>(())) });
        let joined = tasks.join_next().await;
        let result = completion_error(joined, true);
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("http_grpc_server_stopped".to_owned())
        );
    }

    #[tokio::test]
    async fn failed_listener_stops_and_joins_its_sibling() {
        let (http_shutdown, _http_signal) = tokio::sync::oneshot::channel();
        let (grpc_shutdown, grpc_signal) = tokio::sync::oneshot::channel();
        let mut tasks = JoinSet::new();
        tasks.spawn(async {
            (
                "http",
                Err(Error::custom("listener_failed", "HTTP listener failed")),
            )
        });
        tasks.spawn(async move {
            let _ = grpc_signal.await;
            ("grpc", Ok(()))
        });

        let first_exit = stop_on_signal_or_listener_exit(
            &mut tasks,
            std::future::pending(),
            http_shutdown,
            grpc_shutdown,
        )
        .await;
        let first_error = first_exit.and_then(|joined| completion_error(joined, true).err());
        assert_eq!(
            first_error.map(|error| error.code().to_owned()),
            Some("listener_failed".to_owned())
        );
        let sibling = tasks.join_next().await;
        assert!(completion_error(sibling, false).is_ok());
        assert!(tasks.join_next().await.is_none());
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use crate::core::ProviderBuilder;
    use figment::{Figment, providers::Serialized};

    #[tokio::test]
    async fn joint_provider_requires_both_capabilities() {
        let mut builder = ProviderBuilder::new();
        builder.provider(HttpGrpcServiceProvider);
        assert!(
            builder
                .start(Figment::from(Serialized::defaults(
                    crate::config::FrameworkConfig::default()
                )))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn joint_provider_rejects_empty_registry() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder
            .provider(crate::http::HttpServiceProvider)
            .provider(crate::grpc::GrpcServiceProvider::new())
            .provider(HttpGrpcServiceProvider);
        let result = builder
            .start(Figment::from(Serialized::defaults(
                crate::config::FrameworkConfig::default(),
            )))
            .await;
        assert!(result.is_err());
        Ok(())
    }
}
