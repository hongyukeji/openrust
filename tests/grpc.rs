use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("openrust-grpc-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn actual_artisan_installs_grpc_registration_and_preserves_user_routes() {
    let root = TestRoot::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["Cargo.toml", "routes/mod.rs", "bootstrap/app.rs"] {
        let target = root.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(source.join(path), target).unwrap();
    }
    let install = || {
        let output = Command::new(env!("CARGO_BIN_EXE_main"))
            .env("APP_BASE_PATH", &root.0)
            .args(["artisan", "install:grpc"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    install();
    let startup = fs::read_to_string(root.0.join("bootstrap/app.rs")).unwrap();
    assert_eq!(
        startup
            .matches(".with_grpc(routes::grpc::register)")
            .count(),
        1
    );
    let modules = fs::read_to_string(root.0.join("routes/mod.rs")).unwrap();
    assert_eq!(modules.matches("pub(crate) mod grpc;").count(), 1);
    let routes = root.0.join("routes/grpc.rs");
    let mut contents = fs::read_to_string(&routes).unwrap();
    assert!(contents.contains("services: &mut RoutesBuilder"));
    contents.push_str("\n// Existing application code.\n");
    fs::write(&routes, &contents).unwrap();
    install();
    assert_eq!(fs::read_to_string(routes).unwrap(), contents);
    assert_eq!(
        fs::read_to_string(root.0.join("bootstrap/app.rs")).unwrap(),
        startup
    );
}

#[cfg(unix)]
mod process {
    use std::{
        io::{BufRead as _, BufReader},
        net::{SocketAddr, TcpListener},
        process::{Child, Command, Stdio},
        time::Duration,
    };

    use openrust::{
        foundation::Application,
        grpc::{RoutesBuilder, tonic},
        support::ProviderResult,
    };
    use serde_json::json;
    use tonic_health::pb::{
        HealthCheckRequest, health_check_response::ServingStatus, health_client::HealthClient,
    };

    fn configure(
        _: &std::path::Path,
        _: &openrust::config::Environment,
    ) -> Result<
        std::collections::BTreeMap<String, openrust::config::Value>,
        openrust::config::ConfigError,
    > {
        Ok([
            ("app".into(), json!({"host":"127.0.0.1","port":0})),
            (
                "grpc".into(),
                json!({"host":std::env::var("GRPC_TEST_HOST").unwrap(), "port":0}),
            ),
        ]
        .into())
    }

    fn register(_: &Application, routes: &mut RoutesBuilder) -> ProviderResult {
        use openrust::grpc::{Grpc, tonic::transport::ServerTlsConfig};
        if let Some(root) = std::env::var_os("GRPC_TEST_TLS_MATERIAL").map(std::path::PathBuf::from)
        {
            let options = Grpc::server()?;
            options.set_credentials(
                ServerTlsConfig::new()
                    .identity(identity(&root, "server")?)
                    .client_ca_root(root_ca(&root)?),
            );
            options.add_interceptor(|mut request| {
                if request
                    .metadata()
                    .get("x-grpc-step")
                    .is_none_or(|value| value != "client-3")
                {
                    return Err(tonic::Status::unauthenticated("missing metadata"));
                }
                request
                    .metadata_mut()
                    .insert("x-grpc-step", "server-1".parse().unwrap());
                Ok(request)
            });
            options.add_interceptor(|request| {
                if request
                    .metadata()
                    .get("x-grpc-step")
                    .is_none_or(|value| value != "server-1")
                {
                    return Err(tonic::Status::failed_precondition("out of order"));
                }
                Ok(request)
            });
        }
        let (_, health) = tonic_health::server::health_reporter();
        routes.add_service(health);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "spawned by the gRPC process acceptance test"]
    async fn grpc_entry_fixture() {
        Application::configure(std::env::temp_dir())
            .with_config(configure)
            .with_grpc(register)
            .create()
            .unwrap()
            .start()
            .await
            .unwrap();
    }

    struct Process(Child);

    impl Drop for Process {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    fn start(host: &str) -> Process {
        start_server(host, None)
    }

    fn start_server(host: &str, tls: Option<&std::path::Path>) -> Process {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--ignored",
                "--exact",
                "process::grpc_entry_fixture",
                "--nocapture",
            ])
            .env("GRPC_TEST_HOST", host)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(tls) = tls {
            child.env("GRPC_TEST_TLS_MATERIAL", tls);
        } else {
            child.env_remove("GRPC_TEST_TLS_MATERIAL");
        }
        Process(child.spawn().unwrap())
    }

    fn readiness(server: &mut Process) -> (SocketAddr, SocketAddr) {
        let stdout = server.0.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = send.send(line);
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut http = None;
        let mut grpc = None;
        while http.is_none() || grpc.is_none() {
            let line = receive
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            if let Some(addr) = line
                .strip_prefix("OpenRust gRPC listening on http://")
                .or_else(|| line.strip_prefix("OpenRust gRPC listening on https://"))
            {
                grpc = Some(addr.parse().unwrap());
            }
            if let Some(addr) = line.strip_prefix("OpenRust listening on http://") {
                http = Some(addr.parse().unwrap());
            }
        }
        (http.unwrap(), grpc.unwrap())
    }

    #[tokio::test]
    async fn actual_grpc_client_receives_statuses_and_both_ports_release_on_shutdown() {
        let mut server = start("127.0.0.1");
        let (http, grpc) = readiness(&mut server);
        assert_ne!(http, grpc);
        let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{grpc}"))
            .unwrap()
            .timeout(Duration::from_secs(3));
        let mut client = HealthClient::new(endpoint.connect().await.unwrap());
        let success = client
            .check(HealthCheckRequest { service: "".into() })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(success.status(), ServingStatus::Serving);
        let missing = client
            .check(HealthCheckRequest {
                service: "missing".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::NotFound);
        drop(client);

        assert!(
            Command::new("kill")
                .args(["-TERM", &server.0.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let exit = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(exit) = server.0.try_wait().unwrap() {
                    return exit;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if !exit.success() {
            use std::io::Read as _;
            let mut stderr = String::new();
            server
                .0
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            panic!("gRPC fixture exited with {exit}: {stderr}");
        }
        assert!(TcpListener::bind(http).is_ok());
        assert!(TcpListener::bind(grpc).is_ok());
    }

    #[test]
    fn occupied_grpc_port_fails_before_http_reports_readiness() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut server = start(&address.to_string());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while server.0.try_wait().unwrap().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "gRPC port conflict was not reported"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        use std::io::Read as _;
        let mut stdout = String::new();
        let mut stderr = String::new();
        server
            .0
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        server
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.contains("cannot bind gRPC listener"), "{stderr}");
        assert!(!stdout.contains("OpenRust listening on"));
    }
    fn register_client_route() -> Result<(), openrust::routing::RoutingError> {
        use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};
        fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
            Box::pin(async {
                let channel = openrust::grpc::Grpc::connect("health").map_err(|source| {
                    HttpError::Internal {
                        source: Box::new(source),
                    }
                })?;
                let status = HealthClient::new(channel)
                    .check(HealthCheckRequest { service: "".into() })
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?
                    .into_inner()
                    .status();
                Response::json(StatusCode::OK, json!({"status":status.as_str_name()}))
            })
        }
        openrust::support::facades::Route::get("/grpc-probe", probe)?;
        Ok(())
    }

    struct GrpcProbe(openrust::console::CommandDefinition);

    impl openrust::console::Command for GrpcProbe {
        fn definition(&self) -> &openrust::console::CommandDefinition {
            &self.0
        }

        fn handle<'a>(
            &'a self,
            _: &'a Application,
            _: &'a openrust::console::Input,
            output: &'a mut openrust::console::Output,
        ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>>
        {
            Box::pin(async move {
                let fail = |source: &dyn std::error::Error| {
                    openrust::console::ConsoleError::new(source.to_string())
                };
                let channel =
                    openrust::grpc::Grpc::connect("health").map_err(|error| fail(&error))?;
                let status = HealthClient::new(channel)
                    .check(HealthCheckRequest { service: "".into() })
                    .await
                    .map_err(|error| fail(&error))?
                    .into_inner()
                    .status();
                output.comment(status.as_str_name());
                Ok(i32::from(status != ServingStatus::Serving))
            })
        }
    }

    fn register_probe(
        artisan: &mut openrust::console::Application,
    ) -> Result<(), openrust::console::ConsoleError> {
        artisan.add_command(Box::new(GrpcProbe(
            openrust::console::CommandDefinition::new(clap::Command::new("grpc:probe")),
        )))
    }

    async fn client_fixture(peer: SocketAddr) -> Application {
        use openrust::contracts::foundation::Application as _;
        let mut app = Application::configure(std::env::temp_dir())
            .with_routing(
                openrust::foundation::configuration::Routing::default().web(register_client_route),
            )
            .with_commands(register_probe)
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let unused = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        app.config()
            .unwrap()
            .set(
                "grpc.clients",
                json!({
                        "health":{"host":peer.ip().to_string(), "port":peer.port().to_string()},
                        "inlined":{"host":peer.to_string()},
                        "uri":{"endpoint":format!("http://{peer}")},
                        "bad":{"host":"127.0.0.1", "port":-1},
                        "bad_uri":{"endpoint":format!("http://{peer}/path")},
                "bad_port_uri":{"endpoint":"http://127.0.0.1:0"},
                        "closed":{"endpoint":format!("http://{unused}"),"connect_timeout_ms":500}
                    }),
            )
            .unwrap();
        app
    }

    #[tokio::test]
    async fn named_clients_work_in_http_and_artisan() {
        use openrust::{
            contracts::foundation::Application as _,
            grpc::{GrpcClientError, GrpcClients},
        };
        use std::sync::Arc;
        let mut server = start("127.0.0.1");
        let (_, peer) = readiness(&mut server);
        let app = client_fixture(peer).await;
        let registry = app.make::<GrpcClients>().await.unwrap();
        assert!(Arc::ptr_eq(&registry, &app.make().await.unwrap()));
        for name in ["health", "inlined", "uri"] {
            let result = HealthClient::new(registry.connect(name).unwrap())
                .check(HealthCheckRequest { service: "".into() })
                .await
                .unwrap();
            assert_eq!(result.into_inner().status(), ServingStatus::Serving);
        }
        assert!(matches!(
            registry.connect("absent"),
            Err(GrpcClientError::UnknownClient(_))
        ));
        for name in ["bad", "bad_uri", "bad_port_uri"] {
            assert!(matches!(
                registry.connect(name),
                Err(GrpcClientError::InvalidConfig { .. })
            ));
        }
        let failure = tokio::time::timeout(
            Duration::from_secs(3),
            HealthClient::new(registry.connect("closed").unwrap())
                .check(HealthCheckRequest { service: "".into() }),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(failure.code(), tonic::Code::Unavailable);

        app.config()
            .unwrap()
            .set("grpc.clients.health", json!({"host":""}))
            .unwrap();
        assert_http_probe(app).await;
        let artisan = client_fixture(peer).await;
        assert_eq!(
            artisan
                .handle_command(&["grpc:probe".into()])
                .await
                .unwrap(),
            0
        );
    }

    async fn assert_http_probe(app: Application) {
        use axum::{extract::State, handler::Handler as _};
        use openrust::{contracts::http::Kernel as _, foundation::http::Kernel};
        use std::sync::Arc;
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let http = tokio::spawn(async move {
            async fn request(
                State(kernel): State<Arc<Kernel>>,
                request: axum::extract::Request,
            ) -> axum::response::Response {
                kernel.handle_transport(request).await
            }
            axum::serve(
                listener,
                request.with_state(Arc::new(kernel)).into_make_service(),
            )
            .await
            .unwrap();
        });
        let response = tokio::process::Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--max-time",
                "5",
                &format!("http://{address}/grpc-probe"),
            ])
            .output()
            .await
            .unwrap();
        http.abort();
        assert!(http.await.unwrap_err().is_cancelled());
        assert!(response.status.success(), "{response:?}");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.stdout).unwrap(),
            json!({"status":"SERVING"})
        );
    }

    fn root_ca(root: &std::path::Path) -> std::io::Result<tonic::transport::Certificate> {
        Ok(tonic::transport::Certificate::from_pem(std::fs::read(
            root.join("ca.pem"),
        )?))
    }

    fn identity(root: &std::path::Path, name: &str) -> std::io::Result<tonic::transport::Identity> {
        Ok(tonic::transport::Identity::from_pem(
            std::fs::read(root.join(format!("{name}.pem")))?,
            std::fs::read(root.join(format!("{name}.key")))?,
        ))
    }

    fn certificates() -> super::TestRoot {
        let root = super::TestRoot::new();
        let openssl = |args: &[&str]| {
            let result = Command::new("openssl")
                .current_dir(&root.0)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "openssl: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        openssl(&[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-x509",
            "-days",
            "2",
            "-subj",
            "/CN=OpenRust Probe CA",
            "-keyout",
            "ca.key",
            "-out",
            "ca.pem",
        ]);
        for (name, usages) in [
            (
                "server",
                "subjectAltName=DNS:localhost
extendedKeyUsage=serverAuth
",
            ),
            (
                "client",
                "extendedKeyUsage=clientAuth
",
            ),
        ] {
            let request = format!("{name}.csr");
            let key = format!("{name}.key");
            let certificate = format!("{name}.pem");
            std::fs::write(root.0.join("extensions"), usages).unwrap();
            openssl(&[
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-subj",
                "/CN=localhost",
                "-keyout",
                &key,
                "-out",
                &request,
            ]);
            openssl(&[
                "x509",
                "-req",
                "-in",
                &request,
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-extfile",
                "extensions",
                "-days",
                "2",
                "-out",
                &certificate,
            ]);
        }
        root
    }

    async fn secure_client_fixture(root: &std::path::Path, peer: SocketAddr) -> Application {
        use openrust::{contracts::foundation::Application as _, grpc::GrpcClients};
        use tonic::transport::ClientTlsConfig;
        let app = client_fixture(peer).await;
        let clients = app.make::<GrpcClients>().await.unwrap();
        let ca = root_ca(root).unwrap();
        let trust = ClientTlsConfig::new()
            .ca_certificate(ca)
            .domain_name("localhost");
        let authenticated = trust.clone().identity(identity(root, "client").unwrap());
        clients.register_credentials("mtls", authenticated.clone());
        clients.register_credentials("wrong-domain", authenticated.domain_name("wrong.example"));
        clients.register_credentials("no-identity", trust);
        clients.register_credentials(
            "untrusted-ca",
            ClientTlsConfig::new()
                .domain_name("localhost")
                .identity(identity(root, "client").unwrap()),
        );
        clients.register_interceptor("first", |mut request| {
            request
                .metadata_mut()
                .insert("x-grpc-step", "client-1".parse().unwrap());
            Ok(request)
        });
        for (group, expected, next) in [
            ("first", "client-1", "client-2"),
            ("second", "client-2", "client-3"),
        ] {
            clients.register_interceptor(group, move |mut request| {
                if request
                    .metadata()
                    .get("x-grpc-step")
                    .is_none_or(|step| step != expected)
                {
                    return Err(tonic::Status::failed_precondition("out of order"));
                }
                request
                    .metadata_mut()
                    .insert("x-grpc-step", next.parse().unwrap());
                Ok(request)
            });
        }
        clients.register_interceptor("denied", |_| {
            Err(tonic::Status::permission_denied("client rejected"))
        });
        let config = app.config().unwrap();
        for (name, group, interceptors) in [
            ("health", "mtls", vec!["first", "unused", "second"]),
            ("absent-group", "missing", vec![]),
            ("wrong-domain", "wrong-domain", vec![]),
            ("no-identity", "no-identity", vec![]),
            ("untrusted-ca", "untrusted-ca", vec![]),
            ("missing-metadata", "mtls", vec![]),
            ("client-denied", "mtls", vec!["denied"]),
        ] {
            config
                .set(
                    &format!("grpc.clients.{name}"),
                    json!({
                        "host":peer.to_string(), "credentials": group, "interceptors":interceptors,
                        "connect_timeout_ms":1000, "timeout_ms":2000,
                    }),
                )
                .unwrap();
        }
        config
            .set(
                "grpc.clients.bad-scheme",
                json!({"endpoint":format!("http://{peer}"),"credentials":"mtls"}),
            )
            .unwrap();
        config
            .set(
                "grpc.clients.invalid-interceptors",
                json!({"host":peer.to_string(),"interceptors":"first"}),
            )
            .unwrap();
        app
    }

    #[tokio::test]
    async fn mtls_and_interceptors_work_in_http_and_artisan() {
        use openrust::grpc::{GrpcClientError, GrpcClients};
        let material = certificates();
        let mut server = start_server("127.0.0.1", Some(&material.0));
        let (_, peer) = readiness(&mut server);
        let app = secure_client_fixture(&material.0, peer).await;
        let connections = app.make::<GrpcClients>().await.unwrap();
        for name in ["absent-group", "bad-scheme", "invalid-interceptors"] {
            assert!(matches!(
                connections.connect(name),
                Err(GrpcClientError::InvalidConfig { .. })
            ));
        }
        for (name, code) in [
            ("wrong-domain", None),
            ("no-identity", None),
            ("untrusted-ca", None),
            ("missing-metadata", Some(tonic::Code::Unauthenticated)),
            ("client-denied", Some(tonic::Code::PermissionDenied)),
        ] {
            let status = tokio::time::timeout(
                Duration::from_secs(4),
                HealthClient::new(connections.connect(name).unwrap())
                    .check(HealthCheckRequest { service: "".into() }),
            )
            .await
            .unwrap()
            .unwrap_err();
            if let Some(code) = code {
                assert_eq!(status.code(), code, "{name}: {status:?}");
            } else {
                use std::error::Error as _;
                assert!(
                    matches!(
                        status.code(),
                        tonic::Code::Unavailable | tonic::Code::Unknown
                    ) && status.source().is_some(),
                    "{name}: {status:?}"
                );
            }
        }

        assert_http_probe(app).await;
        let artisan = secure_client_fixture(&material.0, peer).await;
        assert_eq!(
            artisan
                .handle_command(&["grpc:probe".into()])
                .await
                .unwrap(),
            0
        );
    }
}
