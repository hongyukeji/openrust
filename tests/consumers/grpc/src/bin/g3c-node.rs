use std::{env, net::SocketAddr, pin::Pin, time::Duration};

use futures_util::{Stream, stream};
use openrust::{
    config::{FrameworkConfig, GrpcConfig},
    core::{App, BootContext, BoxFuture, ProviderBuilder, ServiceProvider},
    grpc::{GrpcServer, GrpcServiceProvider, GrpcServiceRegistry, tonic},
};
use tokio::net::TcpListener;
use tonic::{Request, Response, Status};

pub mod proto {
    tonic::include_proto!("openrust.example.v1");
}

use proto::{
    EchoRequest, EchoResponse,
    echo_server::{Echo, EchoServer},
};

#[derive(Clone)]
struct NodeService {
    id: String,
}

#[tonic::async_trait]
impl Echo for NodeService {
    type ServerStreamStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;
    type BidirectionalStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

    async fn unary(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        Ok(Response::new(EchoResponse {
            value: request.into_inner().value,
            api_revision: self.id.clone(),
        }))
    }

    async fn server_stream(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        let prefix = request.into_inner().value;
        let node = self.id.clone();
        let responses = stream::unfold(0_u64, move |index| {
            let prefix = prefix.clone();
            let node = node.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Some((
                    Ok(EchoResponse {
                        value: format!("{prefix}-{index}"),
                        api_revision: node,
                    }),
                    index + 1,
                ))
            }
        });
        Ok(Response::new(Box::pin(responses)))
    }

    async fn client_stream(
        &self,
        _request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<EchoResponse>, Status> {
        Err(Status::unimplemented("probe service"))
    }

    async fn bidirectional(
        &self,
        _request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStream>, Status> {
        Err(Status::unimplemented("probe service"))
    }
}

struct NodeProvider(String);
impl ServiceProvider for NodeProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        let id = self.0.clone();
        Box::pin(async move {
            app.extension_mut::<GrpcServiceRegistry>()?
                .register::<EchoServer<NodeService>, _>(move |_| {
                    EchoServer::new(NodeService { id: id.clone() })
                })
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = env::var("G3C_NODE_ID")?;
    let address: SocketAddr = env::var("G3C_LISTEN_ADDR")?.parse()?;
    let listener = TcpListener::bind(address).await?;
    let bound_address = listener.local_addr()?;
    let mut config = FrameworkConfig {
        grpc: GrpcConfig {
            listen_addr: bound_address,
            allow_insecure_server: true,
            health_drain_grace_ms: 4_000,
            ..GrpcConfig::default()
        },
        ..FrameworkConfig::default()
    };
    config.server.shutdown_timeout_secs = 12;
    let mut builder = ProviderBuilder::new();
    builder
        .provider(GrpcServiceProvider::new())
        .provider(NodeProvider(id.clone()));
    let app = App::from_provider_runtime(
        builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(config),
            ))
            .await?,
    )?;
    let server = GrpcServer::from_app(&app).await?;
    println!("NODE_READY={id} ADDRESS={bound_address}");

    let shutdown_node_id = id;
    server
        .serve(listener, async move {
            #[cfg(unix)]
            {
                let Ok(mut terminate) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                else {
                    return;
                };
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = terminate.recv() => {},
                }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
            println!("NODE_SHUTDOWN_SIGNAL={shutdown_node_id}");
        })
        .await?;
    Ok(())
}
