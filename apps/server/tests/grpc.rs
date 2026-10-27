//! 聚合 server package 的 gRPC 插件与 TCP 生命周期验收。

use std::io::{BufRead, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use std::{convert::Infallible, future::Ready, task::Poll};

use openrust::{
    config::FrameworkConfig,
    core::{AppBuilder, Readiness, finalize},
    grpc::{GrpcPlugin, GrpcServer, health_pb, tonic},
    http::{AppBuilderHttpExt, HttpPlugin},
    runtime::{HttpGrpcPlugin, serve_http_and_grpc_until},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
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
    let readiness = app.state().try_get::<Readiness>()?;

    assert!(app.commands().any(|command| command.name() == "grpc serve"));
    let server = GrpcServer::from_app(&app)?;
    assert_eq!(server.listen_addr(), "127.0.0.1:50051".parse()?);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (shutdown, shutdown_signal) = oneshot::channel::<()>();
    let task = tokio::spawn(server.serve(listener, async move {
        let _ = shutdown_signal.await;
    }));
    let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
        .connect()
        .await?;
    let mut health = health_pb::health_client::HealthClient::new(channel);
    let report = health
        .check(tonic::Request::new(health_pb::HealthCheckRequest {
            service: "openrust.test.v1.Probe".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(
        report.status,
        health_pb::health_check_response::ServingStatus::Serving as i32
    );
    assert!(readiness.is_ready());
    shutdown
        .send(())
        .map_err(|()| "gRPC server exited before shutdown")?;
    task.await??;
    assert!(!readiness.is_ready());
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
async fn server_host_composes_http_and_grpc_under_one_lifecycle()
-> Result<(), Box<dyn std::error::Error>> {
    let http_probe = TcpListener::bind("127.0.0.1:0").await?;
    let http_address = http_probe.local_addr()?;
    drop(http_probe);
    let grpc_probe = TcpListener::bind("127.0.0.1:0").await?;
    let grpc_address = grpc_probe.local_addr()?;
    drop(grpc_probe);

    let mut config = FrameworkConfig::default();
    config.server.host = http_address.ip().to_string();
    config.server.port = http_address.port();
    config.grpc.listen_addr = grpc_address;
    config.grpc.allow_insecure_server = true;
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)?;
    builder.routes(|routes| {
        routes.get("/trace", || async {
            openrust::core::ExecutionContext::current()
                .and_then(|context| context.metadata().traceparent().map(str::to_owned))
                .unwrap_or_default()
        })
    });
    builder
        .plugin(HttpPlugin)
        .plugin(GrpcPlugin::new())
        .plugin(HttpGrpcPlugin);
    let app = Arc::new(finalize(builder).await?);
    assert!(app.commands().any(|command| command.name() == "serve all"));
    let readiness = app.state().try_get::<Readiness>()?;
    let (stop, signal) = oneshot::channel();
    let running_app = Arc::clone(&app);
    let task = tokio::spawn(async move {
        serve_http_and_grpc_until(&running_app, async move {
            let _ = signal.await;
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !readiness.is_ready() {
            tokio::task::yield_now().await;
        }
    })
    .await?;

    let mut http = TcpStream::connect(http_address).await?;
    http.write_all(
        format!("GET /ready HTTP/1.1\r\nHost: {http_address}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await?;
    let mut response = Vec::new();
    http.read_to_end(&mut response).await?;
    assert!(String::from_utf8(response)?.starts_with("HTTP/1.1 200"));

    let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let mut trace_request = TcpStream::connect(http_address).await?;
    trace_request
        .write_all(
            format!(
                "GET /trace HTTP/1.1\r\nHost: {http_address}\r\ntraceparent: {traceparent}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
    let mut trace_response = Vec::new();
    trace_request.read_to_end(&mut trace_response).await?;
    assert!(String::from_utf8(trace_response)?.contains(traceparent));

    let channel = tonic::transport::Channel::from_shared(format!("http://{grpc_address}"))?
        .connect()
        .await?;
    let mut health = health_pb::health_client::HealthClient::new(channel);
    let health = health
        .check(tonic::Request::new(health_pb::HealthCheckRequest {
            service: "openrust.test.v1.Probe".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(
        health.status,
        health_pb::health_check_response::ServingStatus::Serving as i32
    );

    stop.send(())
        .map_err(|()| "combined service exited before shutdown")?;
    task.await??;
    assert!(!readiness.is_ready());
    assert!(TcpStream::connect(http_address).await.is_err());
    assert!(TcpStream::connect(grpc_address).await.is_err());
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

#[tokio::test]
async fn grpc_serve_signal_path_drains_on_sigterm() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let mut child = Command::new(executable)
        .args(["--exact", "grpc_sigterm_child", "--nocapture"])
        .env("OPENRUST_GRPC_SIGTERM_CHILD", "1")
        .env_remove("RUST_TEST_THREADS")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("child stdout unavailable")?;
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let mut reported_ready = false;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    if !reported_ready {
                        let _ = ready_tx.send(Err(
                            "child gRPC server exited before becoming ready".to_owned(),
                        ));
                    }
                    break;
                }
                Ok(_) if line.starts_with("GRPC_READY ") && !reported_ready => {
                    reported_ready = true;
                    let _ = ready_tx.send(Ok(()));
                }
                Ok(_) => {}
                Err(error) => {
                    if !reported_ready {
                        let _ = ready_tx.send(Err(error.to_string()));
                    }
                    break;
                }
            }
        }
    });
    if !matches!(ready_rx.recv_timeout(Duration::from_secs(30)), Ok(Ok(()))) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("child gRPC server did not become ready within 30 seconds".into());
    }
    let signal = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()?;
    if !signal.success() {
        let _ = child.kill();
        return Err("could not signal child gRPC server".into());
    }
    let waited = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok::<_, std::io::Error>(status);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let status = if let Ok(status) = waited {
        status?
    } else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("child gRPC server did not stop after SIGTERM".into());
    };
    assert!(status.success(), "child gRPC server failed: {status}");
    Ok(())
}

#[tokio::test]
async fn grpc_sigterm_child() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("OPENRUST_GRPC_SIGTERM_CHILD").is_none() {
        return Ok(());
    }
    let mut config = FrameworkConfig::default();
    config.grpc.allow_insecure_server = true;
    config.server.shutdown_timeout_secs = 1;
    let mut builder = AppBuilder::new(config, figment::Figment::new());
    GrpcPlugin::register_service::<ProbeService, _>(&mut builder, |_| ProbeService)?;
    builder.plugin(GrpcPlugin::new());
    let app = finalize(builder).await?;
    let server = GrpcServer::from_app(&app)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let listener_addr = listener.local_addr()?;
    let task = tokio::spawn(server.serve(listener, openrust::core::shutdown_signal()));
    tokio::time::sleep(Duration::from_millis(100)).await;
    if task.is_finished() {
        task.await??;
        return Err("child gRPC server stopped before SIGTERM".into());
    }
    writeln!(std::io::stdout().lock(), "GRPC_READY {listener_addr}")?;
    std::io::stdout().flush()?;
    task.await??;
    Ok(())
}
