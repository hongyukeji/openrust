//! `serve` 命令。

use crate::core::{App, BoxFuture, Command, Error, Result, shutdown_signal};
use std::net::SocketAddr;
use tokio::net::TcpListener;

use crate::http::plugin::build_router;

/// 绑定 `[server] host:port` 并持续提供服务，直到收到关闭信号。
pub async fn serve(app: &App) -> Result<()> {
    let router = build_router(app)?;
    let addr = app.config().server.bind_addr();
    let listener = TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(app = %app.config().app.name, env = %app.config().app.env, "listening on http://{local}");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(Error::internal)
}

/// `serve`：启动 HTTP 服务器。它是每个应用二进制的默认命令。
pub struct ServeCommand;

impl Command for ServeCommand {
    fn name(&self) -> &'static str {
        "serve"
    }

    fn about(&self) -> &'static str {
        "Start the HTTP server"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(serve(app))
    }
}
