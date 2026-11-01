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
    core::{App, BootContext, BoxFuture, ProviderBuilder, RegisterContext, ServiceProvider},
    grpc::{GrpcClientRegistry, GrpcServer, GrpcServiceProvider, GrpcServiceRegistry, tonic},
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::iter;
use tonic::{Request, Response, Status};

struct TokensProvider(openrust::auth::JwtTokens);
impl ServiceProvider for TokensProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(self.0.clone())).await?;
            Ok(())
        })
    }
}
struct RegistryProvider(GrpcServiceRegistry);
impl ServiceProvider for RegistryProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.instance(Arc::new(self.0.clone())).await?;
            Ok(())
        })
    }
}
struct RouteProvider<F>(F);
impl<F: Fn(openrust::http::Routes) -> openrust::http::Routes + Send + Sync + Clone + 'static>
    ServiceProvider for RouteProvider<F>
{
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        let routes = self.0.clone();
        Box::pin(async move {
            app.routes(routes)?;
            Ok(())
        })
    }
}
fn sources(config: FrameworkConfig) -> figment::Figment {
    figment::Figment::from(figment::providers::Serialized::defaults(config))
}
async fn start(builder: ProviderBuilder, sources: figment::Figment) -> openrust::Result<App> {
    App::from_provider_runtime(builder.start(sources).await?)
}

pub mod proto {
    tonic::include_proto!("openrust.example.v1");
}

pub mod legacy_proto {
    include!(concat!(env!("OUT_DIR"), "/legacy/openrust.example.v1.rs"));
}

use proto::{
    EchoRequest, EchoResponse,
    echo_client::EchoClient,
    echo_server::{Echo, EchoServer},
};

#[derive(Clone)]
struct EchoService {
    dropped_streams: Arc<AtomicUsize>,
    policy: Option<openrust::auth::Policy>,
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
        let value = if request.get_ref().value == "__whoami" {
            let user = openrust::grpc::GrpcAuth::current_user(&request)
                .map_err(|error| openrust::grpc::GrpcAuth::status(&error))?;
            format!("{}:{}", user.id(), user.has_ability("posts:read"))
        } else if request.get_ref().value == "__authorize" {
            let policy = self
                .policy
                .as_ref()
                .ok_or_else(|| Status::internal("policy unavailable"))?;
            openrust::grpc::GrpcAuth::authorize(&request, policy, "posts:read")
                .await
                .map_err(|error| openrust::grpc::GrpcAuth::status(&error))?;
            "allowed".to_owned()
        } else if request.get_ref().value == "__traceparent" {
            openrust::core::ExecutionContext::current()
                .and_then(|context| context.metadata().traceparent().map(str::to_owned))
                .unwrap_or_default()
        } else {
            request.get_ref().value.clone()
        };
        Ok(Response::new(EchoResponse {
            value,
            api_revision: "v2".to_owned(),
        }))
    }

    async fn server_stream(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<Self::ServerStreamStream>, Status> {
        let prefix = request.into_inner().value;
        let guard = StreamDrop(Arc::clone(&self.dropped_streams));
        if prefix == "__slow" {
            let responses =
                stream::unfold((prefix, 0, guard), |(prefix, index, guard)| async move {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Some((
                        Ok(EchoResponse {
                            value: format!("{prefix}-{index}"),
                            api_revision: "v2".to_owned(),
                        }),
                        (prefix, index + 1, guard),
                    ))
                });
            return Ok(Response::new(Box::pin(responses)));
        }
        let responses = stream::unfold((prefix, 0, guard), |(prefix, index, guard)| async move {
            if index == 3 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some((
                Ok(EchoResponse {
                    value: format!("{prefix}-{index}"),
                    api_revision: "v2".to_owned(),
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
            api_revision: "v2".to_owned(),
        }))
    }

    async fn bidirectional(
        &self,
        request: Request<tonic::Streaming<EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStream>, Status> {
        let responses = request.into_inner().map(|request| {
            request.map(|request| EchoResponse {
                value: request.value.to_uppercase(),
                api_revision: "v2".to_owned(),
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
            ..GrpcConfig::default()
        },
        ..FrameworkConfig::default()
    };

    let config = sources(config);
    let mut builder = ProviderBuilder::new();
    let mut registry = GrpcServiceRegistry::default();
    let dropped_streams = Arc::new(AtomicUsize::new(0));
    let service_dropped_streams = Arc::clone(&dropped_streams);
    registry.register_with_config::<EchoServer<EchoService>, _>(move |_, config| {
        EchoServer::new(EchoService {
            dropped_streams: Arc::clone(&service_dropped_streams),
            policy: None,
        })
        .max_decoding_message_size(config.max_decoding_message_size_bytes)
        .max_encoding_message_size(config.max_encoding_message_size_bytes)
    })?;
    builder
        .provider(RegistryProvider(registry))
        .provider(GrpcServiceProvider::new());
    let app = start(builder, config).await?;
    let server = GrpcServer::from_app(&app).await?;
    let clients = Arc::clone(&app.state().resolve::<GrpcClientRegistry>().await?);
    let (shutdown, signal) = oneshot::channel::<()>();
    let server_task = tokio::spawn(server.serve(listener, async move {
        let _ = signal.await;
    }));

    let mut client = EchoClient::new(clients.channel(None)?);
    let unary = client
        .unary(Request::new(EchoRequest {
            value: "hello".to_owned(),
            client_hint: String::new(),
        }))
        .await?
        .into_inner();
    assert_eq!(unary.value, "hello");
    let named = EchoClient::new(clients.channel(Some("echo"))?)
        .unary(Request::new(EchoRequest {
            value: "named".to_owned(),
            client_hint: String::new(),
        }))
        .await?
        .into_inner();
    assert_eq!(named.value, "named");

    let mut server_stream = client
        .server_stream(Request::new(EchoRequest {
            value: "item".to_owned(),
            client_hint: String::new(),
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
                client_hint: String::new(),
            },
            EchoRequest {
                value: "two".to_owned(),
                client_hint: String::new(),
            },
        ]))
        .await?
        .into_inner();
    assert_eq!(client_stream.value, "one,two");

    let mut bidirectional = client
        .bidirectional(iter([
            EchoRequest {
                value: "first".to_owned(),
                client_hint: String::new(),
            },
            EchoRequest {
                value: "second".to_owned(),
                client_hint: String::new(),
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
            client_hint: String::new(),
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
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        let dropped = Arc::new(AtomicUsize::new(0));
        registry.register::<EchoServer<EchoService>, _>(move |_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::clone(&dropped),
                policy: None,
            })
        })?;
        builder
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let app = start(builder, config).await?;
        let server = GrpcServer::from_app(&app).await?;
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
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let registry = GrpcServiceRegistry::default();
        builder
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let app = start(builder, config).await?;
        let clients = Arc::clone(&app.state().resolve::<GrpcClientRegistry>().await?);
        Ok(EchoClient::new(clients.channel(None)?))
    }

    async fn call_unary(client: &mut EchoClient<Channel>) -> Result<String, Box<dyn StdError>> {
        let response = tokio::time::timeout(
            Duration::from_secs(3),
            client.unary(Request::new(EchoRequest {
                value: "tls".to_owned(),
                client_hint: String::new(),
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

#[cfg(test)]
mod auth_tests {
    use super::*;
    use openrust::{
        auth::{
            AuthCredential, AuthPrincipal, AuthServiceProvider, AuthSource, Authenticator,
            JwtTokens,
        },
        config::JwtConfig,
        core::{BoxFuture, Error},
    };
    use tonic::{Code, metadata::MetadataValue, transport::Channel};

    struct FakePatAuthenticator;

    impl Authenticator for FakePatAuthenticator {
        fn authenticate<'a>(
            &'a self,
            credential: AuthCredential<'a>,
        ) -> BoxFuture<'a, openrust::Result<AuthPrincipal>> {
            Box::pin(async move {
                if credential.source() == AuthSource::PersonalAccessToken
                    && credential.value() == Some("openrust_pat_test")
                {
                    Ok(AuthPrincipal::new(91, Some(vec!["posts:read".to_owned()])))
                } else {
                    Err(Error::Unauthorized)
                }
            })
        }
    }

    #[tokio::test]
    async fn jwt_bearer_authentication_runs_over_real_grpc_tcp()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let jwt = JwtConfig::new(
            "https://grpc-tests.example",
            "grpc-tests",
            "active",
            [(
                "active".to_owned(),
                "U1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1M=".to_owned(),
            )]
            .into(),
        );
        let mut config = FrameworkConfig::default();
        config.grpc.listen_addr = address;
        config.grpc.allow_insecure_server = true;
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        registry.register_with_config::<EchoServer<EchoService>, _>(|_, config| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
            .max_decoding_message_size(config.max_decoding_message_size_bytes)
            .max_encoding_message_size(config.max_encoding_message_size_bytes)
        })?;
        builder
            .provider(TokensProvider(JwtTokens::new(jwt)?))
            .provider(AuthServiceProvider::with_authenticator(
                FakePatAuthenticator,
            ))
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let app = start(builder, config).await?;
        let token = app
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(73)?;
        let server = GrpcServer::from_app(&app).await?;
        let (shutdown, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(server.serve(listener, async move {
            let _ = signal.await;
        }));
        let channel = Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);

        let missing = client
            .unary(Request::new(EchoRequest {
                value: "__whoami".into(),
                client_hint: String::new(),
            }))
            .await
            .expect_err("missing bearer must fail at the protected method");
        assert_eq!(missing.code(), Code::Unauthenticated);

        let mut invalid = Request::new(EchoRequest {
            value: "__whoami".into(),
            client_hint: String::new(),
        });
        invalid.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer invalid-token")?,
        );
        assert_eq!(
            client
                .unary(invalid)
                .await
                .expect_err("invalid bearer must fail in the server layer")
                .code(),
            Code::Unauthenticated
        );

        let mut authenticated = Request::new(EchoRequest {
            value: "__whoami".into(),
            client_hint: String::new(),
        });
        authenticated.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}"))?,
        );
        assert_eq!(
            client.unary(authenticated).await?.into_inner().value,
            "73:false"
        );

        let mut pat = Request::new(EchoRequest {
            value: "__whoami".into(),
            client_hint: String::new(),
        });
        pat.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer openrust_pat_test")?,
        );
        assert_eq!(client.unary(pat).await?.into_inner().value, "91:true");

        let mut duplicate = Request::new(EchoRequest {
            value: "__whoami".into(),
            client_hint: String::new(),
        });
        duplicate.metadata_mut().append(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}"))?,
        );
        duplicate.metadata_mut().append(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}"))?,
        );
        assert_eq!(
            client
                .unary(duplicate)
                .await
                .expect_err("duplicate auth headers must fail")
                .code(),
            Code::Unauthenticated
        );

        let mut cookie_only = Request::new(EchoRequest {
            value: "__whoami".into(),
            client_hint: String::new(),
        });
        cookie_only.metadata_mut().insert(
            "cookie",
            MetadataValue::try_from("openrust_session=ignored")?,
        );
        assert_eq!(
            client
                .unary(cookie_only)
                .await
                .expect_err("Cookie Session is not RPC auth")
                .code(),
            Code::Unauthenticated
        );

        shutdown
            .send(())
            .map_err(|()| "gRPC auth server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use openrust::grpc::{health_pb, reflection_pb};
    use openrust::{
        http::HttpServiceProvider,
        runtime::{HttpGrpcServiceProvider, serve_http_and_grpc_until},
    };
    use std::{error::Error as StdError, net::SocketAddr, sync::Arc};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::JoinHandle;
    use tokio::{net::TcpStream, sync::Notify};
    use tonic::Code;

    struct LegacyEchoService;

    #[tonic::async_trait]
    impl legacy_proto::echo_server::Echo for LegacyEchoService {
        type ServerStreamStream =
            Pin<Box<dyn Stream<Item = Result<legacy_proto::EchoResponse, Status>> + Send>>;
        type BidirectionalStream =
            Pin<Box<dyn Stream<Item = Result<legacy_proto::EchoResponse, Status>> + Send>>;

        async fn unary(
            &self,
            request: Request<legacy_proto::EchoRequest>,
        ) -> Result<Response<legacy_proto::EchoResponse>, Status> {
            Ok(Response::new(legacy_proto::EchoResponse {
                value: request.into_inner().value,
            }))
        }

        async fn server_stream(
            &self,
            request: Request<legacy_proto::EchoRequest>,
        ) -> Result<Response<Self::ServerStreamStream>, Status> {
            let response = legacy_proto::EchoResponse {
                value: request.into_inner().value,
            };
            Ok(Response::new(Box::pin(tokio_stream::iter([Ok(response)]))))
        }

        async fn client_stream(
            &self,
            request: Request<tonic::Streaming<legacy_proto::EchoRequest>>,
        ) -> Result<Response<legacy_proto::EchoResponse>, Status> {
            let mut stream = request.into_inner();
            let mut values = Vec::new();
            while let Some(request) = stream.message().await? {
                values.push(request.value);
            }
            Ok(Response::new(legacy_proto::EchoResponse {
                value: values.join(","),
            }))
        }

        async fn bidirectional(
            &self,
            request: Request<tonic::Streaming<legacy_proto::EchoRequest>>,
        ) -> Result<Response<Self::BidirectionalStream>, Status> {
            let stream = request.into_inner().map(|item| {
                item.map(|request| legacy_proto::EchoResponse {
                    value: request.value,
                })
            });
            Ok(Response::new(Box::pin(stream)))
        }
    }

    async fn start_runtime_server(
        config: FrameworkConfig,
        reflection: bool,
    ) -> Result<
        (
            SocketAddr,
            oneshot::Sender<()>,
            JoinHandle<openrust::core::Result<()>>,
        ),
        Box<dyn StdError>,
    > {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let mut config = config;
        config.grpc.listen_addr = address;
        config.grpc.allow_insecure_server = true;
        config.grpc.reflection_enabled = reflection;
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        registry.register_with_config::<EchoServer<EchoService>, _>(|_, config| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
            .max_decoding_message_size(config.max_decoding_message_size_bytes)
            .max_encoding_message_size(config.max_encoding_message_size_bytes)
        })?;
        if reflection {
            registry.register_reflection_descriptor_set(
                include_bytes!(concat!(env!("OUT_DIR"), "/echo_descriptor.bin")).to_vec(),
                ["openrust.example.v1.Echo"],
            )?;
        }
        builder
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let app = start(builder, config).await?;
        let server = GrpcServer::from_app(&app).await?;
        let (stop, signal) = oneshot::channel();
        let task = tokio::spawn(server.serve(listener, async move {
            let _ = signal.await;
        }));
        Ok((address, stop, task))
    }

    async fn unused_address() -> Result<SocketAddr, Box<dyn StdError>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        Ok(listener.local_addr()?)
    }

    #[tokio::test]
    async fn old_and_new_proto_clients_interoperate_over_real_tcp() -> Result<(), Box<dyn StdError>>
    {
        let config = FrameworkConfig {
            grpc: openrust::config::GrpcConfig {
                allow_insecure_server: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let (address, stop, current_task) = start_runtime_server(config, false).await?;

        let mut old_client =
            legacy_proto::echo_client::EchoClient::connect(format!("http://{address}")).await?;
        let old_response = old_client
            .unary(legacy_proto::EchoRequest {
                value: "old-client".to_owned(),
            })
            .await?
            .into_inner();
        assert_eq!(old_response.value, "old-client");
        drop(old_client);

        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let mut current_client = EchoClient::connect(format!("http://{address}")).await?;
        let trace_context = openrust::core::ExecutionContext::new(
            openrust::core::ExecutionMetadata::default().with_traceparent(traceparent)?,
            None,
            openrust::core::Cancellation::new(),
        );
        let trace_response = openrust::core::with_execution_context(trace_context, async {
            let mut request = Request::new(EchoRequest {
                value: "__traceparent".to_owned(),
                client_hint: String::new(),
            });
            assert!(GrpcClientRegistry::propagate_trace_context(&mut request));
            current_client.unary(request).await
        })
        .await?;
        assert_eq!(trace_response.into_inner().value, traceparent);
        let mut invalid_trace_request = Request::new(EchoRequest {
            value: "__traceparent".to_owned(),
            client_hint: String::new(),
        });
        invalid_trace_request
            .metadata_mut()
            .insert("traceparent", "invalid".parse()?);
        let invalid_trace_response = current_client.unary(invalid_trace_request).await?;
        assert!(invalid_trace_response.into_inner().value.is_empty());
        drop(current_client);
        let _ = stop.send(());
        current_task.await??;

        let legacy_listener = TcpListener::bind("127.0.0.1:0").await?;
        let legacy_address = legacy_listener.local_addr()?;
        let (stop_legacy, signal_legacy) = oneshot::channel();
        let legacy_task = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(legacy_proto::echo_server::EchoServer::new(
                    LegacyEchoService,
                ))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(legacy_listener),
                    async move {
                        let _ = signal_legacy.await;
                    },
                ),
        );
        let mut current_client = EchoClient::connect(format!("http://{legacy_address}")).await?;
        let current_response = current_client
            .unary(EchoRequest {
                value: "new-client".to_owned(),
                client_hint: "ignored-by-legacy-server".to_owned(),
            })
            .await?
            .into_inner();
        assert_eq!(current_response.value, "new-client");
        assert_eq!(current_response.api_revision, "");
        let _ = stop_legacy.send(());
        legacy_task.await??;
        Ok(())
    }

    async fn combined_app(
        http_address: SocketAddr,
        grpc_address: SocketAddr,
        shutdown_timeout_secs: u64,
        blocked_request: Option<Arc<Notify>>,
    ) -> Result<Arc<openrust::App>, Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.server.host = http_address.ip().to_string();
        config.server.port = http_address.port();
        config.server.shutdown_timeout_secs = shutdown_timeout_secs;
        config.grpc.listen_addr = grpc_address;
        config.grpc.allow_insecure_server = true;
        openrust::core::lifecycle::init_tracing(&config.log);
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        registry.register_with_config::<EchoServer<EchoService>, _>(|_, grpc| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
            .max_decoding_message_size(grpc.max_decoding_message_size_bytes)
            .max_encoding_message_size(grpc.max_encoding_message_size_bytes)
        })?;
        if let Some(blocked_request) = blocked_request {
            builder.provider(RouteProvider(move |routes: openrust::http::Routes| {
                let blocked_request = Arc::clone(&blocked_request);
                routes.get("/blocked", move || {
                    let blocked_request = Arc::clone(&blocked_request);
                    async move {
                        blocked_request.notify_one();
                        std::future::pending::<&'static str>().await
                    }
                })
            }));
        }
        let grpc_channel =
            tonic::transport::Endpoint::from_shared(format!("http://{grpc_address}"))?
                .connect_lazy();
        builder.provider(RouteProvider(move |routes: openrust::http::Routes| {
            let grpc_channel = grpc_channel.clone();
            routes.get("/trace", move || {
                let mut client = EchoClient::new(grpc_channel.clone());
                async move {
                    let mut request = Request::new(EchoRequest {
                        value: "__traceparent".to_owned(),
                        client_hint: String::new(),
                    });
                    if !GrpcClientRegistry::propagate_trace_context(&mut request) {
                        return "trace-missing".to_owned();
                    }
                    client
                        .unary(request)
                        .await
                        .map(|response| response.into_inner().value)
                        .unwrap_or_else(|_| "rpc-failed".to_owned())
                }
            })
        }));
        builder
            .provider(HttpServiceProvider)
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new())
            .provider(HttpGrpcServiceProvider);
        Ok(Arc::new(start(builder, config).await?))
    }

    async fn wait_until_ready(
        readiness: openrust::core::Readiness,
    ) -> Result<(), Box<dyn StdError>> {
        tokio::time::timeout(Duration::from_secs(5), async move {
            while !readiness.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    async fn http_request(address: SocketAddr, path: &str) -> Result<String, Box<dyn StdError>> {
        let mut stream = TcpStream::connect(address).await?;
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nhost: {address}\r\nconnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(String::from_utf8(response)?)
    }

    #[tokio::test]
    async fn grpc_deadline_covers_stream_lifetime_and_global_limit_holds_until_end()
    -> Result<(), Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.grpc.max_concurrent_rpcs = 1;
        config.grpc.default_timeout_secs = 2;
        config.grpc.max_timeout_secs = 3;
        let (address, stop, task) = start_runtime_server(config, false).await?;
        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);
        let mut stream_request = Request::new(EchoRequest {
            value: "deadline".to_owned(),
            client_hint: String::new(),
        });
        stream_request
            .metadata_mut()
            .insert("grpc-timeout", "55m".parse()?);
        let mut stream = client.server_stream(stream_request).await?.into_inner();
        assert_eq!(
            stream
                .message()
                .await?
                .ok_or("first response missing")?
                .value,
            "deadline-0"
        );
        assert_eq!(
            stream
                .message()
                .await?
                .ok_or("second response missing")?
                .value,
            "deadline-1"
        );

        let overloaded = client
            .unary(Request::new(EchoRequest {
                value: "overloaded".to_owned(),
                client_hint: String::new(),
            }))
            .await
            .expect_err("global RPC capacity must include an active response stream");
        assert_eq!(overloaded.code(), Code::ResourceExhausted);

        let expired = stream
            .message()
            .await
            .expect_err("a stream exceeding its original grpc-timeout must terminate");
        assert_eq!(expired.code(), Code::DeadlineExceeded);
        assert_eq!(
            client
                .unary(Request::new(EchoRequest {
                    value: "released".to_owned(),
                    client_hint: String::new(),
                }))
                .await?
                .into_inner()
                .value,
            "released"
        );
        stop.send(())
            .map_err(|()| "gRPC server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn reflection_is_opt_in_and_lists_only_registered_descriptor_services()
    -> Result<(), Box<dyn StdError>> {
        let (address, stop, task) = start_runtime_server(FrameworkConfig::default(), true).await?;
        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client =
            reflection_pb::v1::server_reflection_client::ServerReflectionClient::new(channel);
        let request = reflection_pb::v1::ServerReflectionRequest {
            host: String::new(),
            message_request: Some(
                reflection_pb::v1::server_reflection_request::MessageRequest::ListServices(
                    String::new(),
                ),
            ),
        };
        let mut responses = client
            .server_reflection_info(tonic::Request::new(iter([request])))
            .await?
            .into_inner();
        let response = responses
            .message()
            .await?
            .ok_or("reflection response missing")?;
        let Some(
            reflection_pb::v1::server_reflection_response::MessageResponse::ListServicesResponse(
                services,
            ),
        ) = response.message_response
        else {
            return Err("reflection did not return its service list".into());
        };
        assert!(
            services
                .service
                .iter()
                .any(|service| service.name == "openrust.example.v1.Echo")
        );
        stop.send(())
            .map_err(|()| "gRPC server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn health_watch_reports_not_serving_before_graceful_drain()
    -> Result<(), Box<dyn StdError>> {
        let (address, stop, task) = start_runtime_server(FrameworkConfig::default(), false).await?;
        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut health = health_pb::health_client::HealthClient::new(channel);
        let mut watch = health
            .watch(tonic::Request::new(health_pb::HealthCheckRequest {
                service: "openrust.example.v1.Echo".to_owned(),
            }))
            .await?
            .into_inner();
        let serving = watch
            .message()
            .await?
            .ok_or("initial health status missing")?;
        assert_eq!(
            serving.status,
            health_pb::health_check_response::ServingStatus::Serving as i32
        );
        stop.send(())
            .map_err(|()| "gRPC server stopped unexpectedly")?;
        let stopping = watch
            .message()
            .await?
            .ok_or("shutdown health status missing")?;
        assert_eq!(
            stopping.status,
            health_pb::health_check_response::ServingStatus::NotServing as i32
        );
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn reflection_requires_a_descriptor_set_when_enabled() -> Result<(), Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.grpc.reflection_enabled = true;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        config.grpc.listen_addr = listener.local_addr()?;
        config.grpc.allow_insecure_server = true;
        let config = sources(config);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        registry.register::<EchoServer<EchoService>, _>(|_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
        })?;
        builder
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let error = match start(builder, config).await {
            Ok(_) => return Err("reflection unexpectedly enabled without a descriptor".into()),
            Err(error) => error,
        };
        assert_eq!(error.code(), "grpc_reflection_descriptor_missing");
        Ok(())
    }

    #[tokio::test]
    async fn configured_message_size_limit_rejects_oversized_requests_over_tcp()
    -> Result<(), Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.grpc.max_decoding_message_size_bytes = 8;
        let (address, stop, task) = start_runtime_server(config, false).await?;
        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);
        let error = client
            .unary(Request::new(EchoRequest {
                value: "larger than the configured protobuf frame".to_owned(),
                client_hint: String::new(),
            }))
            .await
            .expect_err("the configured generated-server decoding limit must apply");
        assert_eq!(error.code(), Code::OutOfRange);
        stop.send(())
            .map_err(|()| "gRPC server stopped unexpectedly")?;
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn shutdown_timeout_cancels_and_joins_a_non_terminating_stream()
    -> Result<(), Box<dyn StdError>> {
        let mut config = FrameworkConfig::default();
        config.server.shutdown_timeout_secs = 1;
        let (address, stop, task) = start_runtime_server(config, false).await?;
        let channel = tonic::transport::Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);
        let mut stream = client
            .server_stream(Request::new(EchoRequest {
                value: "__slow".to_owned(),
                client_hint: String::new(),
            }))
            .await?
            .into_inner();
        assert!(stream.message().await?.is_some());
        stop.send(())
            .map_err(|()| "gRPC server stopped unexpectedly")?;
        let result = task.await?;
        let error = result.expect_err("unbounded stream must hit the configured drain limit");
        assert_eq!(error.code(), "grpc_shutdown_timeout");
        drop(stream);
        Ok(())
    }

    #[tokio::test]
    async fn combined_runtime_serves_both_protocols_and_drains_together()
    -> Result<(), Box<dyn StdError>> {
        let http_address = unused_address().await?;
        let grpc_address = unused_address().await?;
        let app = combined_app(http_address, grpc_address, 2, None).await?;
        assert!(app.commands().any(|command| command.name() == "serve all"));
        let readiness = app.state().try_get::<openrust::core::Readiness>()?;
        let (stop, signal) = oneshot::channel();
        let serving_app = Arc::clone(&app);
        let task = tokio::spawn(async move {
            serve_http_and_grpc_until(&serving_app, async move {
                let _ = signal.await;
            })
            .await
        });

        wait_until_ready((*readiness).clone()).await?;
        let response = http_request(http_address, "/ready").await?;
        assert!(response.starts_with("HTTP/1.1 200"));
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let mut trace_stream = TcpStream::connect(http_address).await?;
        trace_stream
            .write_all(
                format!(
                    "GET /trace HTTP/1.1\r\nhost: {http_address}\r\ntraceparent: {traceparent}\r\nconnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        let mut trace_response = Vec::new();
        trace_stream.read_to_end(&mut trace_response).await?;
        assert!(String::from_utf8(trace_response)?.contains(traceparent));
        let channel = tonic::transport::Channel::from_shared(format!("http://{grpc_address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);
        assert_eq!(
            client
                .unary(Request::new(EchoRequest {
                    value: "combined".to_owned(),
                    client_hint: String::new(),
                }))
                .await?
                .into_inner()
                .value,
            "combined"
        );

        stop.send(())
            .map_err(|()| "combined server stopped unexpectedly")?;
        task.await??;
        assert!(!readiness.is_ready());
        assert!(TcpStream::connect(http_address).await.is_err());
        assert!(TcpStream::connect(grpc_address).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn combined_startup_binding_failure_releases_the_first_listener()
    -> Result<(), Box<dyn StdError>> {
        let occupied_grpc_listener = TcpListener::bind("127.0.0.1:0").await?;
        let grpc_address = occupied_grpc_listener.local_addr()?;
        let http_address = unused_address().await?;
        let app = combined_app(http_address, grpc_address, 2, None).await?;
        let (_stop, signal) = oneshot::channel::<()>();
        let error = serve_http_and_grpc_until(&app, async move {
            let _ = signal.await;
        })
        .await
        .expect_err("gRPC bind conflict must fail before serving HTTP");
        assert_eq!(error.code(), "internal_error");
        assert!(TcpStream::connect(http_address).await.is_err());
        drop(occupied_grpc_listener);
        Ok(())
    }

    #[tokio::test]
    async fn combined_shutdown_timeout_aborts_and_joins_both_listeners()
    -> Result<(), Box<dyn StdError>> {
        let http_address = unused_address().await?;
        let grpc_address = unused_address().await?;
        let blocked_request = Arc::new(Notify::new());
        let app = combined_app(
            http_address,
            grpc_address,
            1,
            Some(Arc::clone(&blocked_request)),
        )
        .await?;
        let readiness = app.state().try_get::<openrust::core::Readiness>()?;
        let (stop, signal) = oneshot::channel();
        let serving_app = Arc::clone(&app);
        let task = tokio::spawn(async move {
            serve_http_and_grpc_until(&serving_app, async move {
                let _ = signal.await;
            })
            .await
        });
        wait_until_ready((*readiness).clone()).await?;
        let mut request = TcpStream::connect(http_address).await?;
        request
            .write_all(format!("GET /blocked HTTP/1.1\r\nHost: {http_address}\r\n\r\n").as_bytes())
            .await?;
        tokio::time::timeout(Duration::from_secs(5), blocked_request.notified()).await?;
        stop.send(())
            .map_err(|()| "combined server stopped unexpectedly")?;
        let result = task.await?;
        let error = result.expect_err("blocked HTTP request must hit the joint deadline");
        assert_eq!(error.code(), "http_grpc_shutdown_timeout");
        assert!(!readiness.is_ready());
        assert!(TcpStream::connect(http_address).await.is_err());
        assert!(TcpStream::connect(grpc_address).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn combined_plugin_rejects_missing_http_or_grpc_registration()
    -> Result<(), Box<dyn StdError>> {
        let config = sources(FrameworkConfig::default());
        let mut builder = ProviderBuilder::new();
        let registry = GrpcServiceRegistry::default();
        builder
            .provider(HttpServiceProvider)
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new())
            .provider(HttpGrpcServiceProvider);
        let error = match start(builder, config).await {
            Ok(_) => {
                return Err("joint runtime unexpectedly accepted an empty service registry".into());
            }
            Err(error) => error,
        };
        assert_eq!(error.code(), "grpc_service_missing");

        let config = sources(FrameworkConfig::default());
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        registry.register::<EchoServer<EchoService>, _>(|_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
        })?;
        builder
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new())
            .provider(HttpGrpcServiceProvider);
        let error = match start(builder, config).await {
            Ok(_) => {
                return Err("joint runtime unexpectedly accepted a missing HTTP plugin".into());
            }
            Err(error) => error,
        };
        assert_eq!(error.code(), "http_grpc_dependency_missing");
        Ok(())
    }
}

#[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
mod database_auth_tests {
    use super::*;
    use figment::providers::Format;
    use openrust::{
        auth::{
            AuthMigration, AuthPolicyMigration, AuthServiceProvider, JwtConfig, JwtTokens,
            PersonalAccessTokens, Policy,
        },
        database::{
            DatabaseBackend, DatabaseServiceProvider,
            sea_orm::ConnectionTrait,
            sea_orm_migration::{MigrationTrait, MigratorTrait},
        },
    };
    use std::error::Error as StdError;
    use testing::TestDatabase;
    use tonic::{Code, metadata::MetadataValue, transport::Channel};

    struct AuthMigrator;

    impl MigratorTrait for AuthMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            vec![Box::new(AuthMigration), Box::new(AuthPolicyMigration)]
        }
    }

    #[tokio::test]
    async fn database_bearer_and_method_policy_work_over_real_tcp() -> Result<(), Box<dyn StdError>>
    {
        let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("跳过 gRPC 双库认证验收：未设置 OPENRUST_TEST_DATABASE_URL");
            return Ok(());
        };
        let backend = DatabaseBackend::from_url(&admin_url)?;
        if !backend.is_enabled() {
            eprintln!("跳过 gRPC 双库认证验收：当前 feature 未启用 URL 对应数据库");
            return Ok(());
        }
        let database = TestDatabase::create::<AuthMigrator>(&admin_url).await?;
        let users = openrust::db::DatabaseTableNames::default()
            .quote("users", database.connection()?.get_database_backend())?;
        database.connection()?.execute_unprepared(&format!(
            "INSERT INTO {users} (email, password_hash, created_at, updated_at) VALUES ('grpc-auth@example.test', 'unused', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
        )).await?;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let key = "U1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1NTU1M=".to_owned();
        let jwt = JwtConfig::new(
            "https://grpc-db-tests.example",
            "grpc-db-tests",
            "active",
            [("active".to_owned(), key.clone())].into(),
        );
        let source = format!("[database]\nurl = {:?}\n", database.url());
        let figment = figment::Figment::new().merge(figment::providers::Toml::string(&source));
        let mut config = FrameworkConfig::default();
        config.grpc.listen_addr = address;
        config.grpc.allow_insecure_server = true;
        let config = sources(config).merge(figment);
        let mut builder = ProviderBuilder::new();
        let mut registry = GrpcServiceRegistry::default();
        let policy_for_service = Arc::new(std::sync::OnceLock::<Policy>::new());
        let policy_factory = Arc::clone(&policy_for_service);
        registry.register::<EchoServer<EchoService>, _>(move |_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: policy_factory.get().cloned(),
            })
        })?;
        builder
            .provider(DatabaseServiceProvider)
            .provider(TokensProvider(JwtTokens::new(jwt)?))
            .provider(AuthServiceProvider::new())
            .provider(RegistryProvider(registry))
            .provider(GrpcServiceProvider::new());
        let app = start(builder, config).await?;
        let jwt_token = app
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(1)?;
        let policy = app.state().resolve::<Policy>().await?;
        policy_for_service
            .set((*policy).clone())
            .map_err(|_| "policy was already initialized")?;
        let role = policy.ensure_role("grpc-reader").await?;
        policy.assign_role(1, role).await?;
        policy.grant_permission(role, "posts:read").await?;
        let pats = app.state().resolve::<PersonalAccessTokens>().await?;
        let pat = pats
            .issue(1, "grpc-reader", &["posts:read".to_owned()], None)
            .await?;
        let denied_pat = pats.issue(1, "grpc-denied", &[], None).await?;

        let server = GrpcServer::from_app(&app).await?;
        let (shutdown, signal) = oneshot::channel::<()>();
        let server_task = tokio::spawn(server.serve(listener, async move {
            let _ = signal.await;
        }));
        let channel = Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        let mut client = EchoClient::new(channel);

        let mut jwt_request = Request::new(EchoRequest {
            value: "__authorize".into(),
            client_hint: String::new(),
        });
        jwt_request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {jwt_token}"))?,
        );
        assert_eq!(
            client.unary(jwt_request).await?.into_inner().value,
            "allowed"
        );

        let mut pat_request = Request::new(EchoRequest {
            value: "__authorize".into(),
            client_hint: String::new(),
        });
        pat_request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", pat.expose()))?,
        );
        assert_eq!(
            client.unary(pat_request).await?.into_inner().value,
            "allowed"
        );

        let mut denied_request = Request::new(EchoRequest {
            value: "__authorize".into(),
            client_hint: String::new(),
        });
        denied_request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", denied_pat.expose()))?,
        );
        assert_eq!(
            client
                .unary(denied_request)
                .await
                .expect_err("PAT without ability must be denied")
                .code(),
            Code::PermissionDenied
        );

        let issued = pats.list_for_user(1).await?;
        let token_id = issued
            .iter()
            .find(|item| item.name == "grpc-reader")
            .ok_or("issued PAT metadata missing")?
            .id;
        assert!(pats.revoke(1, token_id).await?);
        let mut revoked_request = Request::new(EchoRequest {
            value: "__authorize".into(),
            client_hint: String::new(),
        });
        revoked_request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", pat.expose()))?,
        );
        assert_eq!(
            client
                .unary(revoked_request)
                .await
                .expect_err("revoked PAT must be rejected")
                .code(),
            Code::Unauthenticated
        );

        shutdown
            .send(())
            .map_err(|()| "gRPC database auth server stopped unexpectedly")?;
        server_task.await??;
        drop(pats);
        drop(policy);
        drop(app);
        database.close().await?;
        Ok(())
    }
}
