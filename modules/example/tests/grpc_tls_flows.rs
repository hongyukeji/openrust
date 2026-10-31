//! 同一生产应用的 HTTP/gRPC 双端口组合与真实 TLS；不是同端口或集群验收。
#![cfg(feature = "grpc")]

use super::grpc::echo::{EchoRequest, echo_client::EchoClient};
use super::services::ExampleMessage;
use figment::{Figment, providers::Serialized};
use openrust::config::GrpcServerTlsConfig;
use openrust::core::Readiness;
use openrust::grpc::tonic;
use openrust::testing::TestApp;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

const BUDGET: Duration = Duration::from_secs(5);
const MESSAGE: &str = "Shared HTTP and TLS RPC binding";
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct ShutdownWitness;
struct Authority {
    pem: String,
    issuer: Issuer<'static, KeyPair>,
}
fn authority(name: &str) -> TestResult<Authority> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    let key = KeyPair::generate()?;
    let certificate = params.self_signed(&key)?;
    Ok(Authority {
        pem: certificate.pem(),
        issuer: Issuer::new(params, key),
    })
}
struct Materials {
    _directory: tempfile::TempDir,
    tls: GrpcServerTlsConfig,
    ca: String,
    unrelated_ca: String,
}
impl Materials {
    fn create() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let ca = authority("Trusted Test CA")?;
        let other = authority("Unrelated Test CA")?;
        let mut params = CertificateParams::new(vec!["localhost".into()])?;
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        let key = KeyPair::generate()?;
        let certificate = params.signed_by(&key, &ca.issuer)?;
        let certificate_file = directory.path().join("server.pem");
        let private_key_file = directory.path().join("server-key.pem");
        std::fs::write(&certificate_file, certificate.pem())?;
        std::fs::write(&private_key_file, key.serialize_pem())?;
        Ok(Self {
            _directory: directory,
            tls: GrpcServerTlsConfig {
                certificate_file,
                private_key_file,
                client_ca_certificate_file: None,
            },
            ca: ca.pem,
            unrelated_ca: other.pem,
        })
    }
}
struct Running {
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<openrust::Result<()>>>,
}
impl Running {
    async fn stop(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(mut task) = self.task.take() {
            if let Ok(result) = tokio::time::timeout(BUDGET, &mut task).await {
                result??;
            } else {
                task.abort();
                let _ = tokio::time::timeout(BUDGET, task).await;
                return Err("combined listener shutdown exceeded deadline".into());
            }
        }
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
fn endpoint(address: SocketAddr, ca: &str, domain: &str) -> TestResult<tonic::transport::Endpoint> {
    Ok(
        tonic::transport::Endpoint::from_shared(format!("https://{address}"))?
            .connect_timeout(BUDGET)
            .timeout(BUDGET)
            .tls_config(
                tonic::transport::ClientTlsConfig::new()
                    .ca_certificate(tonic::transport::Certificate::from_pem(ca))
                    .domain_name(domain),
            )?,
    )
}
#[derive(Clone, Copy, Debug)]
enum TlsRejection {
    UnknownIssuer,
    WrongName,
}

fn has_certificate_rejection(
    error: &(dyn std::error::Error + 'static),
    expected: TlsRejection,
) -> bool {
    let mut current = Some(error);
    while let Some(cause) = current {
        // tonic/hyper 会把 rustls 错误转为 boxed transport source，类型向下转换不可用；
        // 仅识别 rustls 的具体证书拒绝标识，普通连接/HTTP2 失败不能算 TLS 负例通过。
        let message = cause.to_string();
        let matched = match expected {
            TlsRejection::UnknownIssuer => message == "invalid peer certificate: UnknownIssuer",
            TlsRejection::WrongName => {
                message
                    == "invalid peer certificate: certificate not valid for name \"wrong.example.test\"; certificate is only valid for DnsName(\"localhost\")"
            }
        };
        if matched {
            return true;
        }
        current = cause.source();
    }
    false
}

#[test]
fn certificate_classifier_rejects_other_failures() {
    for message in [
        "connection refused",
        "operation timed out",
        "transport error",
        "HTTP2 stream error",
        "invalid peer certificate: BadSignature",
        "certificate error: UnknownIssuer",
        "invalid peer certificate: NotValidForNameExtra",
        "invalid peer certificate: certificate not valid for name \"wrong.example.test\"",
    ] {
        let error = std::io::Error::other(message);
        assert!(!has_certificate_rejection(
            &error,
            TlsRejection::UnknownIssuer
        ));
        assert!(!has_certificate_rejection(&error, TlsRejection::WrongName));
    }
    assert!(has_certificate_rejection(
        &std::io::Error::other("invalid peer certificate: UnknownIssuer"),
        TlsRejection::UnknownIssuer
    ));
    assert!(has_certificate_rejection(
        &std::io::Error::other(
            "invalid peer certificate: certificate not valid for name \"wrong.example.test\"; certificate is only valid for DnsName(\"localhost\")"
        ),
        TlsRejection::WrongName
    ));
}

async fn rejected_tls(
    address: SocketAddr,
    ca: &str,
    domain: &str,
    expected: TlsRejection,
) -> TestResult {
    let result = tokio::time::timeout(BUDGET, endpoint(address, ca, domain)?.connect()).await?;
    match result {
        Err(error) => {
            if !has_certificate_rejection(&error, expected) {
                return Err(format!(
                    "TLS connection failed without the expected {expected:?} certificate rejection"
                )
                .into());
            }
        }
        Ok(channel) => {
            // 某些 channel 实现延后握手错误到首次请求。
            let mut client = EchoClient::new(channel);
            let reply = tokio::time::timeout(
                BUDGET,
                client.unary(tonic::Request::new(EchoRequest {
                    value: "untrusted".into(),
                })),
            )
            .await?;
            let error = reply
                .err()
                .ok_or("untrusted TLS identity executed business RPC")?;
            assert!(
                has_certificate_rejection(&error, expected),
                "TLS RPC failed without the expected certificate rejection"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // 同一真实 TLS/HTTP 生命周期包含共享绑定与联合停机证据。
async fn real_tls_and_http_share_module_binding_and_one_supervised_shutdown() -> TestResult {
    let materials = Materials::create()?;
    // 将两个预绑定 listener 直接移交同一公开 supervisor，避免释放随机端口后重绑的所有权空窗。
    let http_listener = TcpListener::bind("127.0.0.1:0").await?;
    let grpc_listener = TcpListener::bind("127.0.0.1:0").await?;
    let http = http_listener.local_addr()?;
    let grpc = grpc_listener.local_addr()?;
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.server.host = http.ip().to_string();
    framework.server.port = http.port();
    framework.server.shutdown_timeout_secs = 3;
    framework.grpc.listen_addr = grpc;
    framework.grpc.tls = Some(materials.tls.clone());
    framework.grpc.allow_insecure_server = false;
    framework.grpc.health_drain_grace_ms = 25;
    let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
        "app":{"env":"test"},
        "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    })));
    let closed = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&closed);
    let mut builder = TestApp::<crate::bootstrap::Server>::builder(source);
    builder.configure_after_register(move |bindings| {
        bindings.instance(Arc::new(ExampleMessage::new(MESSAGE)));
        let closed = Arc::clone(&observed);
        bindings.singleton::<ShutdownWitness, _>(move |resolver| {
            let closed = Arc::clone(&closed);
            Box::pin(async move {
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        closed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })?;
                Ok(Arc::new(ShutdownWitness))
            })
        });
        Ok(())
    });
    let app = Arc::new(builder.build().await?);
    app.app()?.state().resolve::<ShutdownWitness>().await?;
    let readiness = app.app()?.state().resolve::<Readiness>().await?;
    let (stop, signal) = oneshot::channel();
    let running_app = Arc::clone(&app);
    let task = tokio::spawn(async move {
        openrust::runtime::serve_http_and_grpc_with_listeners_until(
            running_app.app()?,
            http_listener,
            grpc_listener,
            async move {
                let _ = signal.await;
            },
        )
        .await
    });
    let mut running = Running {
        stop: Some(stop),
        task: Some(task),
    };
    tokio::time::timeout(BUDGET, async {
        while !readiness.is_ready() {
            if running
                .task
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished)
            {
                return Err("combined service exited before readiness");
            }
            tokio::task::yield_now().await;
        }
        Ok::<(), &str>(())
    })
    .await??;
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(BUDGET)
        .build()?;
    let mut response = http_client
        .get(format!("http://{http}/api/example"))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err("combined HTTP response exceeded budget".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["data"]["message"], MESSAGE);
    let channel = tokio::time::timeout(
        BUDGET,
        endpoint(grpc, &materials.ca, "localhost")?.connect(),
    )
    .await??;
    let mut client = EchoClient::new(channel);
    let reply = tokio::time::timeout(
        BUDGET,
        client.unary(tonic::Request::new(EchoRequest {
            value: "article".into(),
        })),
    )
    .await??;
    assert_eq!(reply.into_inner().value, format!("{MESSAGE}: article"));
    rejected_tls(
        grpc,
        &materials.unrelated_ca,
        "localhost",
        TlsRejection::UnknownIssuer,
    )
    .await?;
    rejected_tls(
        grpc,
        &materials.ca,
        "wrong.example.test",
        TlsRejection::WrongName,
    )
    .await?;
    // 错误握手不能杀死服务；原正确 channel 仍可完成同一业务调用。
    let reply = tokio::time::timeout(
        BUDGET,
        client.unary(tonic::Request::new(EchoRequest {
            value: "after-rejection".into(),
        })),
    )
    .await??;
    assert_eq!(
        reply.into_inner().value,
        format!("{MESSAGE}: after-rejection")
    );
    assert_eq!(closed.load(Ordering::SeqCst), 0);
    drop(client);
    running.stop().await?;
    assert!(!readiness.is_ready());
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    for (role, address) in [("HTTP", http), ("gRPC", grpc)] {
        let connection = tokio::time::timeout(BUDGET, TcpStream::connect(address)).await?;
        let endpoints = connection
            .as_ref()
            .ok()
            .map(|socket| (socket.local_addr(), socket.peer_addr()));
        assert!(
            connection.is_err(),
            "combined {role} listener {address} remained open; successful socket local/peer: {endpoints:?}"
        );
    }
    let mut app = Arc::try_unwrap(app).map_err(|_| "combined runner still owns TestApp")?;
    app.shutdown(tokio::time::Instant::now() + BUDGET).await?;
    assert_eq!(
        closed.load(Ordering::SeqCst),
        1,
        "explicit shutdown must not repeat managed cleanup"
    );
    Ok(())
}
