//! 通过模块内显式生成脚本产生的 gRPC Protobuf 类型。

#[allow(clippy::pedantic)]
pub mod echo {
    include!("generated/echo.rs");
}

use crate::modules::example::services::ExampleMessage;
use openrust::Result;
use openrust::foundation::ApplicationState;
use openrust::foundation::BootContext;
use openrust::grpc::{GrpcServiceRegistry, tonic};
use openrust::support::{BoxFuture, ServiceProvider};

/// 在模块启用时把实际业务问候服务贡献到同一 gRPC 服务目录。
pub(crate) struct ExampleGrpcServiceProvider;

impl ServiceProvider for ExampleGrpcServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut registry = (*app.resolve::<GrpcServiceRegistry>().await?).clone();
            registry.register::<echo::echo_server::EchoServer<GreetingGrpc>, _>(|state| {
                echo::echo_server::EchoServer::new(GreetingGrpc { state })
            })
        })
    }
}

#[derive(Clone)]
struct GreetingGrpc {
    state: ApplicationState,
}

#[tonic::async_trait]
impl echo::echo_server::Echo for GreetingGrpc {
    async fn unary(
        &self,
        request: tonic::Request<echo::EchoRequest>,
    ) -> std::result::Result<tonic::Response<echo::EchoResponse>, tonic::Status> {
        let value = request.into_inner().value;
        let value = value.trim();
        if value.is_empty() || value.chars().count() > 128 {
            return Err(tonic::Status::invalid_argument(
                "问候内容必须为 1 至 128 个字符",
            ));
        }
        let message = self
            .state
            .resolve::<ExampleMessage>()
            .await
            .map_err(|_| tonic::Status::unavailable("模块问候服务不可用"))?;
        Ok(tonic::Response::new(echo::EchoResponse {
            value: format!("{}: {value}", message.text()),
        }))
    }
}
