use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("openrust-grpc-install-{}", std::process::id()));
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
        startup.matches(".withGrpc(routes::grpc::register)").count(),
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
        let (_, health) = tonic_health::server::health_reporter();
        routes.add_service(health);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "spawned by the gRPC process acceptance test"]
    async fn grpc_entry_fixture() {
        Application::configure(std::env::temp_dir())
            .withConfig(configure)
            .withGrpc(register)
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
        Process(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "process::grpc_entry_fixture",
                    "--nocapture",
                ])
                .env("GRPC_TEST_HOST", host)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
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
            if let Some(addr) = line.strip_prefix("OpenRust gRPC listening on http://") {
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
}
