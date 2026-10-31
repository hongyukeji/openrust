//! 验证生成应用的 `serve all` 命令同时运行 HTTP 与真实 TLS Echo RPC。

#[path = "../app/grpc.rs"]
#[allow(dead_code)] // 生产子进程构造 Provider；测试进程只使用生成的 typed client。
mod grpc;

use grpc::echo::{EchoRequest, echo_client::EchoClient};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
};
use std::{future::Future, net::SocketAddr, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    time::{Instant, timeout},
};
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const BUDGET: Duration = Duration::from_secs(10);

struct OwnedServer(Child);

impl Drop for OwnedServer {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

fn certificate_authority(name: &str) -> TestResult<(String, Issuer<'static, KeyPair>)> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.distinguished_name.push(DnType::CommonName, name);
    let key = KeyPair::generate()?;
    let certificate = params.self_signed(&key)?;
    Ok((certificate.pem(), Issuer::new(params, key)))
}

fn endpoint(address: SocketAddr, ca: &str, domain: &str) -> TestResult<Endpoint> {
    Ok(Endpoint::from_shared(format!("https://{address}"))?
        .connect_timeout(BUDGET)
        .timeout(BUDGET)
        .tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca))
                .domain_name(domain),
        )?)
}

async fn http_api(address: SocketAddr) -> TestResult<String> {
    let mut bytes = Vec::new();
    timeout(BUDGET, async {
        let mut stream = TcpStream::connect(address).await?;
        stream
            .write_all(b"GET /api HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        stream.take(16 * 1024 + 1).read_to_end(&mut bytes).await
    })
    .await??;
    if bytes.len() > 16 * 1024 {
        return Err("generated HTTP response exceeded budget".into());
    }
    let response = String::from_utf8(bytes)?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err("generated HTTP endpoint did not return 200".into());
    }
    Ok(response)
}

fn has_certificate_rejection(error: &(dyn std::error::Error + 'static), expected: &str) -> bool {
    let mut current = Some(error);
    while let Some(cause) = current {
        if cause.to_string() == expected {
            return true;
        }
        current = cause.source();
    }
    false
}

async fn rejected_tls(address: SocketAddr, ca: &str, domain: &str, expected: &str) -> TestResult {
    let result = timeout(BUDGET, endpoint(address, ca, domain)?.connect()).await?;
    match result {
        Err(error) if has_certificate_rejection(&error, expected) => Ok(()),
        Err(_) => Err("TLS connection failed without expected certificate rejection".into()),
        Ok(channel) => {
            let mut client = EchoClient::new(channel);
            let result = timeout(
                BUDGET,
                client.unary(tonic::Request::new(EchoRequest {
                    value: "rejected".into(),
                })),
            )
            .await?;
            match result {
                Err(error) if has_certificate_rejection(&error, expected) => Ok(()),
                _ => Err("untrusted TLS connection unexpectedly completed RPC".into()),
            }
        }
    }
}

#[tokio::test]
async fn generated_binary_serve_all_runs_http_and_tls_echo_then_stops() -> TestResult {
    let directory = tempfile::tempdir()?;
    let config_dir = directory.path().join("config");
    std::fs::create_dir(&config_dir)?;
{{database_setup}}{{auth_config}}{{encryption_config}}
    let (trusted_ca, issuer) = certificate_authority("Generated Test CA")?;
    let (untrusted_ca, _) = certificate_authority("Unrelated Test CA")?;
    let mut params = CertificateParams::new(vec!["localhost".into()])?;
    params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    let key = KeyPair::generate()?;
    let certificate = params.signed_by(&key, &issuer)?;
    let cert_file = directory.path().join("server.pem");
    let key_file = directory.path().join("server-key.pem");
    std::fs::write(&cert_file, certificate.pem())?;
    std::fs::write(&key_file, key.serialize_pem())?;

    let http_probe = TcpListener::bind("127.0.0.1:0").await?;
    let grpc_probe = TcpListener::bind("127.0.0.1:0").await?;
    let http_address = http_probe.local_addr()?;
    let grpc_address = grpc_probe.local_addr()?;
    std::fs::write(
        config_dir.join("server.toml"),
        format!(
            "[server]\nhost = '127.0.0.1'\nport = {}\nshutdown_timeout_secs = 3\n",
            http_address.port()
        ),
    )?;
    std::fs::write(
        config_dir.join("grpc.toml"),
        format!(
            "[grpc]\nlisten_addr = '{grpc_address}'\nhealth_drain_grace_ms = 25\n\n[grpc.tls]\ncertificate_file = {}\nprivate_key_file = {}\n",
            serde_json::to_string(&cert_file.to_string_lossy())?,
            serde_json::to_string(&key_file.to_string_lossy())?
        ),
    )?;
    std::fs::write(
        config_dir.join("cache.toml"),
        "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'memory'\n",
    )?;
    std::fs::write(
        config_dir.join("view.toml"),
        format!(
            "[view]\ndirectory = {}\n",
            serde_json::to_string(&format!("{}/resources/views", env!("CARGO_MANIFEST_DIR")))?
        ),
    )?;
    drop(http_probe);
    drop(grpc_probe);

    let mut command = Command::new(env!("CARGO_BIN_EXE_{{binary_name}}"));
    command
        .arg("serve")
        .arg("all")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("APP_CONFIG_DIR", &config_dir)
        .env("APP_ENV", "test")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut server = OwnedServer(command.spawn()?);
    let mut verification = Box::pin(verify_generated_server(
        &mut server,
        http_address,
        grpc_address,
        &trusted_ca,
        &untrusted_ca,
    ));
    let result = timeout(
        Duration::from_secs(60),
        std::future::poll_fn(|context| {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                verification.as_mut().poll(context)
            })) {
                Ok(polled) => polled,
                Err(_) => {
                    std::task::Poll::Ready(Err("generated serve all verification panicked".into()))
                }
            }
        }),
    )
    .await;
    drop(verification);
    if result.is_err() || result.as_ref().is_ok_and(Result::is_err) {
        let _ = server.0.start_kill();
    }
    let stopped: TestResult<std::process::ExitStatus> = match timeout(BUDGET, server.0.wait()).await
    {
        Ok(result) => result.map_err(Into::into),
        Err(_) => {
            let _ = server.0.start_kill();
            match timeout(BUDGET, server.0.wait()).await {
                Ok(result) => result.map_err(Into::into),
                Err(error) => Err(error.into()),
            }
        }
    };
{{database_cleanup}}
    let result: TestResult = result.map_err(Into::into).and_then(|outcome| outcome);
    result?;
    let status = stopped?;
    if !status.success() {
        return Err("generated serve all exited unsuccessfully".into());
    }
    database_result?;
    if TcpStream::connect(http_address).await.is_ok()
        || TcpStream::connect(grpc_address).await.is_ok()
    {
        return Err("generated listener remained open after shutdown".into());
    }
    Ok(())
}

async fn verify_generated_server(
    server: &mut OwnedServer,
    http_address: SocketAddr,
    grpc_address: SocketAddr,
    trusted_ca: &str,
    untrusted_ca: &str,
) -> TestResult {
    let deadline = Instant::now() + BUDGET;
    let http_response = loop {
        if server.0.try_wait()?.is_some() {
            return Err("generated serve all exited before HTTP readiness".into());
        }
        if let Ok(Ok(response)) = timeout(Duration::from_millis(500), http_api(http_address)).await
        {
            break response;
        }
        if Instant::now() >= deadline {
            return Err("generated HTTP listener did not become ready".into());
        }
        tokio::task::yield_now().await;
    };
    if !http_response.contains("\"name\":\"{{binary_name}}\"") {
        return Err("generated HTTP route returned unexpected application".into());
    }
    let channel = timeout(
        BUDGET,
        endpoint(grpc_address, trusted_ca, "localhost")?.connect(),
    )
    .await??;
    let mut client = EchoClient::new(channel);
    let result = timeout(
        BUDGET,
        client.unary(tonic::Request::new(EchoRequest {
            value: "generated".into(),
        })),
    )
    .await??;
    if result.into_inner().value != "generated" {
        return Err("generated Echo RPC returned unexpected value".into());
    }
    rejected_tls(
        grpc_address,
        untrusted_ca,
        "localhost",
        "invalid peer certificate: UnknownIssuer",
    )
    .await?;
    rejected_tls(
        grpc_address,
        trusted_ca,
        "wrong.example.test",
        "invalid peer certificate: certificate not valid for name \"wrong.example.test\"; certificate is only valid for DnsName(\"localhost\")",
    )
    .await?;
    let result = timeout(
        BUDGET,
        client.unary(tonic::Request::new(EchoRequest {
            value: "still-ready".into(),
        })),
    )
    .await??;
    if result.into_inner().value != "still-ready" {
        return Err("generated Echo RPC stopped after rejected client".into());
    }

    let pid = server.0.id().ok_or("generated server process has no ID")?;
    let signalled = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()?;
    if !signalled.success() {
        return Err("failed to signal owned generated server".into());
    }
    Ok(())
}
