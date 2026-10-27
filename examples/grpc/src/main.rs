use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{Stream, StreamExt, TryStreamExt, stream};
use openrust::{
    config::{FrameworkConfig, GrpcConfig},
    core::{AppBuilder, finalize},
    grpc::{GrpcClientRegistry, GrpcPlugin, GrpcServer, tonic},
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::iter;
use tonic::{Request, Response, Status};

pub mod proto {
    tonic::include_proto!("openrust.example.v1");
}

use proto::{
    EchoRequest, EchoResponse,
    echo_client::EchoClient,
    echo_server::{Echo, EchoServer},
};

#[derive(Clone)]
struct EchoService {
    dropped_streams: Arc<AtomicUsize>,
}

struct StreamDrop(Arc<AtomicUsize>);

impl Drop for StreamDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tonic::async_trait]
impl Echo for EchoService {
    type ServerStreamStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;
    type BidirectionalStream = Pin<Box<dyn Stream<Item = Result<EchoResponse, Status>> + Send>>;

    async fn unary(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        Ok(Response::new(EchoResponse {
            value: request.into_inner().value,
        }))
    }

    async fn server_stream(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        let prefix = request.into_inner().value;
        let guard = StreamDrop(Arc::clone(&self.dropped_streams));
        let responses = stream::unfold((prefix, 0, guard), |(prefix, index, guard)| async move {
            if index == 3 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some((
                Ok(EchoResponse {
                    value: format!("{prefix}-{index}"),
                }),
                (prefix, index + 1, guard),
            ))
        });
        Ok(Response::new(Box::pin(responses)))
    }

    async fn client_stream(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<EchoResponse>, Status> {
        let values = request
            .into_inner()
            .try_fold(Vec::new(), |mut values, request| async move {
                values.push(request.value);
                Ok(values)
            })
            .await?;
        Ok(Response::new(EchoResponse {
            value: values.join(","),
        }))
    }

    async fn bidirectional(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStream>, Status> {
        let responses = request.into_inner().map(|request| {
            request.map(|request| EchoResponse {
                value: request.value.to_uppercase(),
            })
        });
        Ok(Response::new(Box::pin(responses)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let config = FrameworkConfig {
        grpc: GrpcConfig {
            listen_addr: address,
            clients: BTreeMap::from([
                ("default".to_owned(), format!("http://{address}")),
                ("echo".to_owned(), format!("http://{address}")),
            ]),
            default_client: Some("default".to_owned()),
        },
        ..FrameworkConfig::default()
    };

    let mut builder = AppBuilder::new(config, figment::Figment::new());
    let dropped_streams = Arc::new(AtomicUsize::new(0));
    let service_dropped_streams = Arc::clone(&dropped_streams);
    GrpcPlugin::register_service::<EchoServer<EchoService>, _>(&mut builder, move |_| {
        EchoServer::new(EchoService {
            dropped_streams: Arc::clone(&service_dropped_streams),
        })
    })?;
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;
    let server = GrpcServer::from_app(&app)?;
    let clients = Arc::clone(&app.state().try_get::<GrpcClientRegistry>()?);
    let (shutdown, signal) = oneshot::channel::<()>();
    let server_task = tokio::spawn(server.serve(listener, async move {
        let _ = signal.await;
    }));

    let mut client = EchoClient::new(clients.channel(None)?);
    let unary = client
        .unary(Request::new(EchoRequest {
            value: "hello".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(unary.value, "hello");
    let named = EchoClient::new(clients.channel(Some("echo"))?)
        .unary(Request::new(EchoRequest {
            value: "named".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(named.value, "named");

    let mut server_stream = client
        .server_stream(Request::new(EchoRequest {
            value: "item".to_owned(),
        }))
        .await?
        .into_inner();
    let mut streamed = Vec::new();
    while let Some(response) = server_stream.message().await? {
        streamed.push(response.value);
    }
    assert_eq!(streamed, ["item-0", "item-1", "item-2"]);

    let client_stream = client
        .client_stream(iter([
            EchoRequest {
                value: "one".to_owned(),
            },
            EchoRequest {
                value: "two".to_owned(),
            },
        ]))
        .await?
        .into_inner();
    assert_eq!(client_stream.value, "one,two");

    let mut bidirectional = client
        .bidirectional(iter([
            EchoRequest {
                value: "first".to_owned(),
            },
            EchoRequest {
                value: "second".to_owned(),
            },
        ]))
        .await?
        .into_inner();
    let mut replies = Vec::new();
    while let Some(response) = bidirectional.message().await? {
        replies.push(response.value);
    }
    assert_eq!(replies, ["FIRST", "SECOND"]);

    let previous_drop_count = dropped_streams.load(Ordering::SeqCst);
    let mut interrupted = client
        .server_stream(Request::new(EchoRequest {
            value: "disconnect".to_owned(),
        }))
        .await?
        .into_inner();
    assert!(interrupted.message().await?.is_some());
    drop(interrupted);
    tokio::time::timeout(Duration::from_secs(2), async {
        while dropped_streams.load(Ordering::SeqCst) == previous_drop_count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;

    drop(server_stream);
    drop(bidirectional);
    shutdown
        .send(())
        .map_err(|()| "gRPC server exited before shutdown")?;
    server_task.await??;
    println!("gRPC TCP 示例已完成四种 RPC、半关闭和中断流清理验收：{address}");
    Ok(())
}
