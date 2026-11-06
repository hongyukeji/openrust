//! 应用的 gRPC Protobuf 类型与示例服务。

pub mod echo {
    tonic::include_proto!("{{package}}.v1");
}

use openrust::Result;
use openrust::foundation::BootContext;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::grpc::{GrpcServiceRegistry, tonic};

/// 将示例 Echo 服务贡献到应用的 gRPC 服务目录。
pub struct EchoServiceProvider;

impl ServiceProvider for EchoServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut registry = (*app.resolve::<GrpcServiceRegistry>().await?).clone();
            registry.register::<echo::echo_server::EchoServer<EchoService>, _>(|_| {
                echo::echo_server::EchoServer::new(EchoService)
            })
        })
    }
}

#[derive(Clone)]
struct EchoService;

#[tonic::async_trait]
impl echo::echo_server::Echo for EchoService {
    async fn unary(
        &self,
        request: tonic::Request<echo::EchoRequest>,
    ) -> std::result::Result<tonic::Response<echo::EchoResponse>, tonic::Status> {
        let value = request.into_inner().value;
        let value = value.trim();
        if value.is_empty() || value.chars().count() > 128 {
            return Err(tonic::Status::invalid_argument(
                "Echo 内容必须为 1 至 128 个字符",
            ));
        }
        Ok(tonic::Response::new(echo::EchoResponse {
            value: value.to_owned(),
        }))
    }
}
