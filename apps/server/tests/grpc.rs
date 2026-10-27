//! 聚合 server package 的 gRPC 插件与 TCP 生命周期验收。

use std::{convert::Infallible, future::Ready, task::Poll};

use openrust::{
    config::FrameworkConfig,
    core::{AppBuilder, finalize},
    grpc::{GrpcPlugin, GrpcServer, tonic},
};
use tokio::{net::TcpListener, sync::oneshot};
use tower::Service;

#[derive(Clone)]
struct ProbeService;

impl tonic::server::NamedService for ProbeService {
    const NAME: &'static str = "openrust.test.v1.Probe";
}

impl Service<tonic::codegen::http::Request<tonic::body::Body>> for ProbeService {
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _context: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: tonic::codegen::http::Request<tonic::body::Body>) -> Self::Future {
        std::future::ready(Ok(tonic::codegen::http::Response::new(
            tonic::body::Body::empty(),
        )))
    }
}

#[tokio::test]
async fn plugin_registers_service_and_serves_a_bound_tcp_listener()
-> Result<(), Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.grpc.allow_insecure_server = true;
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)?;
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;

    assert!(app.commands().any(|command| command.name() == "grpc serve"));
    let server = GrpcServer::from_app(&app)?;
    assert_eq!(server.listen_addr(), "127.0.0.1:50051".parse()?);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown, shutdown_signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.serve(listener, async move {
        let _ = shutdown_signal.await;
    }));
    let connection = tokio::net::TcpStream::connect(address).await?;
    drop(connection);
    shutdown
        .send(())
        .map_err(|()| "gRPC server exited before shutdown")?;
    task.await??;
    Ok(())
}

#[tokio::test]
async fn plugin_rejects_duplicate_services_by_name() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = FrameworkConfig::default();
    config.grpc.allow_insecure_server = true;
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)?;
    let error = GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)
        .err()
        .ok_or("duplicate service registration unexpectedly succeeded")?;
    assert_eq!(error.code(), "grpc_service_duplicate");
    Ok(())
}

#[tokio::test]
async fn server_requires_an_explicit_service_registration() -> Result<(), Box<dyn std::error::Error>>
{
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;
    let error = GrpcServer::from_app(&app)
        .err()
        .ok_or("server construction unexpectedly succeeded without a service")?;
    assert_eq!(error.code(), "grpc_service_missing");
    Ok(())
}

#[tokio::test]
async fn plaintext_server_requires_an_explicit_development_or_proxy_opt_in()
-> Result<(), Box<dyn std::error::Error>> {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)?;
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;
    let error = GrpcServer::from_app(&app)
        .err()
        .ok_or("plaintext gRPC server unexpectedly passed secure defaults")?;
    assert_eq!(error.code(), "grpc_tls_required");
    Ok(())
}
