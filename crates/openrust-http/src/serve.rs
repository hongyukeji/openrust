//! The `serve` command.

use openrust_core::{App, BoxFuture, Command, Error, Result, shutdown_signal};
use tokio::net::TcpListener;

use crate::provider::build_router;

/// Binds `[server] host:port` and serves until a shutdown signal arrives.
pub async fn serve(app: &App) -> Result<()> {
    let router = build_router(app)?;
    let addr = app.config().server.bind_addr();
    let listener = TcpListener::bind(&addr).await?;
    let local = listener.local_addr()?;
    tracing::info!(app = %app.config().app.name, env = %app.config().app.env, "listening on http://{local}");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(Error::internal)
}

/// `serve`: start the HTTP server. Default command of every application binary.
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
