//! 真实 loopback gRPC 客户端调用已启用模块的业务问候服务。

use super::grpc::echo::{EchoRequest, echo_client::EchoClient};
use figment::{Figment, providers::Serialized};
use openrust::grpc::{GrpcServer, tonic};
use openrust::testing::TestApp;
use std::time::Duration;
use tokio::{net::TcpListener, sync::oneshot};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn enabled_module_registers_real_grpc_business_service() -> TestResult {
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "app": {"env":"test"},
            "cache": {"default":"default","stores":{"default":{"driver":"memory"}}},
            "security": {"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}},
            "grpc": {"allow_insecure_server":true}
        })));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(source)
        .build()
        .await?;
    let server = GrpcServer::from_app(app.app()?).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown, signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.serve(listener, async move {
        let _ = signal.await;
    }));
    let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
        .connect()
        .await?;
    let mut client = EchoClient::new(channel);
    let reply = client
        .unary(tonic::Request::new(EchoRequest {
            value: "  article  ".into(),
        }))
        .await?
        .into_inner();
    assert_eq!(reply.value, "Hello from modules/example: article");
    let invalid = client
        .unary(tonic::Request::new(EchoRequest { value: " ".into() }))
        .await
        .err()
        .ok_or("blank gRPC input was accepted")?;
    assert_eq!(invalid.code(), tonic::Code::InvalidArgument);
    shutdown
        .send(())
        .map_err(|()| "gRPC server exited before shutdown")?;
    tokio::time::timeout(Duration::from_secs(5), task).await???;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    Ok(())
}
