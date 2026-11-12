//! gRPC service registration and transport.

pub use tonic::{self, service::RoutesBuilder};

use std::{future::Future, io, sync::Arc};

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{service::Routes, transport::Server};

use crate::{
    config::Value,
    contracts::config::Repository,
    foundation::{Application, RunningService, RuntimeResult, RuntimeService},
    http::HttpFuture,
    support::ProviderResult,
};

pub(crate) type Registrar = fn(&Application, &mut RoutesBuilder) -> ProviderResult;

pub(crate) struct ConfiguredServer {
    routes: Routes,
    config: Arc<dyn Repository>,
}

impl ConfiguredServer {
    pub(crate) fn new(routes: Routes, config: Arc<dyn Repository>) -> Self {
        Self { routes, config }
    }
}

impl RuntimeService for ConfiguredServer {
    fn prepare(
        &self,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> HttpFuture<'_, RuntimeResult<RunningService>> {
        Box::pin(async move {
            let runner = Runner::bind(self.routes.clone(), self.config.as_ref()).await?;
            Ok(Box::pin(async move {
                if let Some(runner) = runner {
                    runner.serve(shutdown.cancelled_owned()).await
                } else {
                    shutdown.cancelled_owned().await;
                    Ok(())
                }
            }) as RunningService)
        })
    }
}

struct Runner {
    listener: TcpListener,
    routes: Routes,
}

impl Runner {
    async fn bind(routes: Routes, config: &dyn Repository) -> io::Result<Option<Self>> {
        let host = match config.get("grpc.host") {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(host)) if host.trim().is_empty() => return Ok(None),
            Some(Value::String(host)) => host,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "grpc.host must be a string",
                ));
            }
        };
        let listener = if host.matches(':').count() == 1 || host.contains("]:") {
            TcpListener::bind(&host).await
        } else {
            let port = config
                .get("grpc.port")
                .and_then(|port| match port {
                    Value::String(port) => port.parse::<u16>().ok(),
                    Value::Number(port) => port.as_u64().and_then(|port| u16::try_from(port).ok()),
                    _ => None,
                })
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "grpc.port must be an integer from 0 to 65535",
                    )
                })?;
            TcpListener::bind((host.trim_start_matches('[').trim_end_matches(']'), port)).await
        }
        .map_err(|error| {
            io::Error::new(error.kind(), format!("cannot bind gRPC listener: {error}"))
        })?;
        Ok(Some(Self { listener, routes }))
    }

    async fn serve(self, shutdown: impl Future<Output = ()>) -> ProviderResult {
        println!(
            "OpenRust gRPC listening on http://{}",
            self.listener.local_addr()?
        );
        Server::builder()
            .add_routes(self.routes)
            .serve_with_incoming_shutdown(TcpListenerStream::new(self.listener), shutdown)
            .await?;
        Ok(())
    }
}
