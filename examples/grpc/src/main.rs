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
        } else {
            request.get_ref().value.clone()
        };
        Ok(Response::new(EchoResponse { value }))
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
            policy: None,
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
                policy: None,
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

#[cfg(test)]
mod auth_tests {
    use super::*;
    use openrust::{
        auth::{
            AuthCredential, AuthPlugin, AuthPrincipal, AuthSource, Authenticator, JwtPlugin,
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
        let mut builder = AppBuilder::new(config, figment::Figment::new());
        GrpcPlugin::register_service::<EchoServer<EchoService>, _>(&mut builder, |_| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: None,
            })
        })?;
        builder
            .plugin(JwtPlugin::with_config(jwt))
            .plugin(AuthPlugin::with_authenticator(FakePatAuthenticator))
            .plugin(GrpcPlugin::new());
        let app = finalize(builder).await?;
        let token = app.state().try_get::<JwtTokens>()?.issue_access_token(73)?;
        let server = GrpcServer::from_app(&app)?;
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
            }))
            .await
            .expect_err("missing bearer must fail at the protected method");
        assert_eq!(missing.code(), Code::Unauthenticated);

        let mut invalid = Request::new(EchoRequest {
            value: "__whoami".into(),
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
        });
        pat.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from("Bearer openrust_pat_test")?,
        );
        assert_eq!(client.unary(pat).await?.into_inner().value, "91:true");

        let mut duplicate = Request::new(EchoRequest {
            value: "__whoami".into(),
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

#[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
mod database_auth_tests {
    use super::*;
    use figment::providers::Format;
    use openrust::{
        auth::{
            AuthMigration, AuthPlugin, AuthPolicyMigration, JwtConfig, JwtPlugin, JwtTokens,
            PersonalAccessTokens, Policy,
        },
        database::{
            DatabaseBackend, DatabasePlugin,
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
        database.connection()?.execute_unprepared(
            "INSERT INTO tb_users (email, password_hash, created_at, updated_at) VALUES ('grpc-auth@example.test', 'unused', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        ).await?;

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
        let mut builder = AppBuilder::new(config, figment);
        GrpcPlugin::register_service::<EchoServer<EchoService>, _>(&mut builder, |state| {
            EchoServer::new(EchoService {
                dropped_streams: Arc::new(AtomicUsize::new(0)),
                policy: state
                    .try_get::<Policy>()
                    .ok()
                    .map(|policy| (*policy).clone()),
            })
        })?;
        builder
            .plugin(DatabasePlugin)
            .plugin(JwtPlugin::with_config(jwt))
            .plugin(AuthPlugin::new())
            .plugin(GrpcPlugin::new());
        let app = finalize(builder).await?;
        let jwt_token = app.state().try_get::<JwtTokens>()?.issue_access_token(1)?;
        let policy = app.state().try_get::<Policy>()?;
        let role = policy.ensure_role("grpc-reader").await?;
        policy.assign_role(1, role).await?;
        policy.grant_permission(role, "posts:read").await?;
        let pats = app.state().try_get::<PersonalAccessTokens>()?;
        let pat = pats
            .issue(1, "grpc-reader", &["posts:read".to_owned()], None)
            .await?;
        let denied_pat = pats.issue(1, "grpc-denied", &[], None).await?;

        let server = GrpcServer::from_app(&app)?;
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
