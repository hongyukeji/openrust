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
            tls: None,
            allow_insecure_server: true,
            clients: BTreeMap::from([
                (
                    "default".to_owned(),
                    openrust::config::GrpcClientConfig {
                        endpoint: format!("http://{address}"),
                        tls: None,
                    },
                ),
                (
                    "echo".to_owned(),
                    openrust::config::GrpcClientConfig {
                        endpoint: format!("http://{address}"),
                        tls: None,
                    },
                ),
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

#[cfg(test)]
mod tls_tests {
    use super::*;
    use openrust::config::{GrpcClientConfig, GrpcClientTlsConfig, GrpcServerTlsConfig};
    use rcgen::{
        BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    };
    use std::{error::Error as StdError, net::SocketAddr, path::Path, time::Duration};
    use tempfile::TempDir;
    use time::{Duration as TimeDuration, OffsetDateTime};
    use tonic::transport::Channel;

    struct CertificateAuthority {
        pem: String,
        issuer: Issuer<'static, KeyPair>,
    }

    fn certificate_authority() -> Result<CertificateAuthority, Box<dyn StdError>> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let key = KeyPair::generate()?;
        let certificate = params.self_signed(&key)?;
        Ok(CertificateAuthority {
            pem: certificate.pem(),
            issuer: Issuer::new(params, key),
        })
    }

    fn issue_certificate(
        issuer: &Issuer<'static, KeyPair>,
        names: &[&str],
        usage: ExtendedKeyUsagePurpose,
        expired: bool,
    ) -> Result<(String, String), Box<dyn StdError>> {
        let mut params = CertificateParams::new(
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
        )?;
        params.extended_key_usages.push(usage);
        if expired {
            let now = OffsetDateTime::now_utc();
            params.not_before = now - TimeDuration::days(3);
            params.not_after = now - TimeDuration::days(1);
        }
        let key = KeyPair::generate()?;
        let certificate = params.signed_by(&key, issuer)?;
        Ok((certificate.pem(), key.serialize_pem()))
    }

    fn write_material(
        directory: &TempDir,
        filename: &str,
        contents: &str,
    ) -> Result<std::path::PathBuf, Box<dyn StdError>> {
        let path = directory.path().join(filename);
        std::fs::write(&path, contents)?;
        Ok(path)
    }

    async fn start_server(
        listener: TcpListener,
        certificate_file: &Path,
        private_key_file: &Path,
        client_ca_certificate_file: Option<&Path>,
    ) -> Result<
        (
            SocketAddr,
            oneshot::Sender<()>,
            tokio::task::JoinHandle<openrust::core::Result<()>>,
        ),
        Box<dyn StdError>,
    > {
        let address = listener.local_addr()?;
        let mut config = FrameworkConfig::default();
        config.grpc.listen_addr = address;
        config.grpc.tls = Some(GrpcServerTlsConfig {
            certificate_file: certificate_file.to_owned(),
            private_key_file: private_key_file.to_owned(),
            client_ca_certificate_file: client_ca_certificate_file.map(Path::to_owned),
        });
        let mut builder = AppBuilder::new(config, figment::Figment::new());
        let dropped = Arc::new(AtomicUsize::new(0));
        GrpcPlugin::register_service::<EchoServer<EchoService>, _>(&mut builder, move |_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::clone(&dropped),
            })
        })?;
        builder.plugin(GrpcPlugin::new());
        let app = finalize(builder).await?;
        let server = GrpcServer::from_app(&app)?;
        let (stop, signal) = oneshot::channel();
        let task = tokio::spawn(server.serve(listener, async move {
            let _ = signal.await;
        }));
        Ok((address, stop, task))
    }

    async fn tls_client(
        address: SocketAddr,
        ca_certificate_file: &Path,
        domain_name: Option<&str>,
        client_identity: Option<(&Path, &Path)>,
    ) -> Result<EchoClient<Channel>, Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.grpc.clients.insert(
            "test".to_owned(),
            GrpcClientConfig {
                endpoint: format!("https://localhost:{}", address.port()),
                tls: Some(GrpcClientTlsConfig {
                    ca_certificate_file: ca_certificate_file.to_owned(),
                    client_certificate_file: client_identity
                        .map(|(certificate, _)| certificate.to_owned()),
                    client_private_key_file: client_identity.map(|(_, key)| key.to_owned()),
                    domain_name: domain_name.map(str::to_owned),
                }),
            },
        );
        config.grpc.default_client = Some("test".to_owned());
        let mut builder = AppBuilder::new(config, figment::Figment::new());
        builder.plugin(GrpcPlugin::new());
        let app = finalize(builder).await?;
        let clients = Arc::clone(&app.state().try_get::<GrpcClientRegistry>()?);
        Ok(EchoClient::new(clients.channel(None)?))
    }

    async fn call_unary(client: &mut EchoClient<Channel>) -> Result<String, Box<dyn StdError>> {
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            client.unary(Request::new(EchoRequest {
                value: "tls".to_owned(),
            })),
        )
        .await??;
        Ok(response.into_inner().value)
    }

    #[tokio::test]
    async fn tls_verifies_ca_and_server_name_over_real_tcp() -> Result<(), Box<dyn StdError>> {
        let directory = TempDir::new()?;
        let authority = certificate_authority()?;
        let (server_certificate, server_key) = issue_certificate(
            &authority.issuer,
            &["localhost"],
            ExtendedKeyUsagePurpose::ServerAuth,
            false,
        )?;
        let ca_path = write_material(&directory, "ca.pem", &authority.pem)?;
        let server_certificate_path =
            write_material(&directory, "server.pem", &server_certificate)?;
        let server_key_path = write_material(&directory, "server-key.pem", &server_key)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let (address, stop, task) =
            start_server(listener, &server_certificate_path, &server_key_path, None).await?;

        let mut trusted = tls_client(address, &ca_path, None, None).await?;
        assert_eq!(call_unary(&mut trusted).await?, "tls");
        let mut wrong_domain = tls_client(address, &ca_path, Some("wrong.example"), None).await?;
        assert!(call_unary(&mut wrong_domain).await.is_err());
        let other_authority = certificate_authority()?;
        let wrong_ca = write_material(&directory, "wrong-ca.pem", &other_authority.pem)?;
        let mut untrusted = tls_client(address, &wrong_ca, None, None).await?;
        assert!(call_unary(&mut untrusted).await.is_err());

        stop.send(())
            .map_err(|()| "TLS server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn mtls_requires_a_trusted_client_certificate() -> Result<(), Box<dyn StdError>> {
        let directory = TempDir::new()?;
        let authority = certificate_authority()?;
        let (server_certificate, server_key) = issue_certificate(
            &authority.issuer,
            &["localhost"],
            ExtendedKeyUsagePurpose::ServerAuth,
            false,
        )?;
        let (client_certificate, client_key) = issue_certificate(
            &authority.issuer,
            &["client"],
            ExtendedKeyUsagePurpose::ClientAuth,
            false,
        )?;
        let ca_path = write_material(&directory, "ca.pem", &authority.pem)?;
        let server_certificate_path =
            write_material(&directory, "server.pem", &server_certificate)?;
        let server_key_path = write_material(&directory, "server-key.pem", &server_key)?;
        let client_certificate_path =
            write_material(&directory, "client.pem", &client_certificate)?;
        let client_key_path = write_material(&directory, "client-key.pem", &client_key)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let (address, stop, task) = start_server(
            listener,
            &server_certificate_path,
            &server_key_path,
            Some(&ca_path),
        )
        .await?;

        let mut valid = tls_client(
            address,
            &ca_path,
            None,
            Some((&client_certificate_path, &client_key_path)),
        )
        .await?;
        assert_eq!(call_unary(&mut valid).await?, "tls");
        let mut missing = tls_client(address, &ca_path, None, None).await?;
        assert!(call_unary(&mut missing).await.is_err());
        let other_authority = certificate_authority()?;
        let (untrusted_certificate, untrusted_key) = issue_certificate(
            &other_authority.issuer,
            &["client"],
            ExtendedKeyUsagePurpose::ClientAuth,
            false,
        )?;
        let untrusted_certificate_path =
            write_material(&directory, "untrusted-client.pem", &untrusted_certificate)?;
        let untrusted_key_path =
            write_material(&directory, "untrusted-client-key.pem", &untrusted_key)?;
        let mut untrusted = tls_client(
            address,
            &ca_path,
            None,
            Some((&untrusted_certificate_path, &untrusted_key_path)),
        )
        .await?;
        assert!(call_unary(&mut untrusted).await.is_err());

        stop.send(())
            .map_err(|()| "mTLS server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn expired_server_certificate_is_rejected() -> Result<(), Box<dyn StdError>> {
        let directory = TempDir::new()?;
        let authority = certificate_authority()?;
        let (expired_certificate, expired_key) = issue_certificate(
            &authority.issuer,
            &["localhost"],
            ExtendedKeyUsagePurpose::ServerAuth,
            true,
        )?;
        let ca_path = write_material(&directory, "ca.pem", &authority.pem)?;
        let certificate_path = write_material(&directory, "expired.pem", &expired_certificate)?;
        let key_path = write_material(&directory, "expired-key.pem", &expired_key)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let (address, stop, task) =
            start_server(listener, &certificate_path, &key_path, None).await?;
        let mut client = tls_client(address, &ca_path, None, None).await?;
        assert!(call_unary(&mut client).await.is_err());
        stop.send(())
            .map_err(|()| "expired-certificate server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }
}
