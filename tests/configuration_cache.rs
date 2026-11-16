use std::{
    fs,
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant, SystemTime},
};

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "openrust-config-cache-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn application(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
    command
        .current_dir(root)
        .env("APP_BASE_PATH", root)
        .env("QUEUE_CONNECTION", "sync")
        .env("APP_DISABLED_RUNNERS", "queue,schedule,grpc,reverb")
        .env_remove("APP_ENV")
        .env_remove("APP_NAME")
        .env_remove("APP_PORT")
        .env_remove("APP_HOST")
        .env_remove("APP_CONFIG_CACHE");
    command
}

fn artisan(root: &Path, args: &[&str]) -> Output {
    application(root)
        .arg("artisan")
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn health(root: &Path, addr: std::net::SocketAddr) -> String {
    let mut child = Running(
        application(root)
            .env("APP_HOST", "127.0.0.1")
            .env("APP_PORT", addr.port().to_string())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Ok(mut stream) = TcpStream::connect(addr) {
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .write_all(b"GET /up HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut body = String::new();
            stream.read_to_string(&mut body).unwrap();
            assert!(body.starts_with("HTTP/1.1 200 OK"), "{body}");
            return body;
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut error = String::new();
            child
                .0
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut error)
                .unwrap();
            panic!("HTTP process exited {status}: {error}");
        }
        assert!(Instant::now() < deadline, "HTTP process failed to listen");
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn real_artisan_and_http_share_the_cached_snapshot() {
    let root = TestRoot::new();
    let base = &root.0;
    let environment = base.join(".env");
    let cache = base.join("bootstrap/cache/config.json");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let write_environment = |name| {
        fs::write(
            &environment,
            format!("APP_NAME={name}\nAPP_PORT={}\n", addr.port()),
        )
        .unwrap();
    };
    write_environment("First");

    assert!(success(artisan(base, &["config:show", "app.name"])).contains("First"));
    assert!(
        success(artisan(base, &["config:cache"])).contains("Configuration cached successfully.")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let snapshot: serde_json::Value = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
    assert_eq!(snapshot["app"]["name"], "First");

    write_environment("Second");
    assert!(success(artisan(base, &["config:show", "app.name"])).contains("First"));
    assert!(health(base, addr).contains("First"));
    assert!(success(artisan(base, &["conf:ca"])).contains("Configuration cached successfully."));
    assert!(success(artisan(base, &["config:show", "app.name"])).contains("Second"));
    assert!(health(base, addr).contains("Second"));

    fs::write(&environment, "APP_NAME=\"unterminated\n").unwrap();
    assert!(success(artisan(base, &["config:show", "app"])).contains("Second"));
    assert!(health(base, addr).contains("Second"));
    let previous = fs::read(&cache).unwrap();
    assert!(!artisan(base, &["config:cache"]).status.success());
    assert_eq!(fs::read(&cache).unwrap(), previous);
    assert!(
        success(artisan(base, &["config:clear"]))
            .contains("Configuration cache cleared successfully.")
    );
    assert!(!cache.exists());
    assert!(!artisan(base, &["config:show", "app.name"]).status.success());

    write_environment("Third");
    assert!(
        success(artisan(base, &["config:clear"]))
            .contains("Configuration cache cleared successfully.")
    );
    assert!(success(artisan(base, &["config:show", "app.name"])).contains("Third"));
    assert!(health(base, addr).contains("Third"));

    fs::write(&cache, "{fixture-private-broken-cache").unwrap();
    let failed = artisan(base, &["config:show", "app.name"]);
    assert_eq!(failed.status.code(), Some(1));
    let diagnostic = String::from_utf8_lossy(&failed.stderr);
    assert!(diagnostic.contains("invalid configuration cache"));
    assert!(!diagnostic.contains("fixture-private-broken-cache"));
    success(artisan(base, &["config:clear"]));
    assert!(!cache.exists());

    let missing = artisan(base, &["config:show", "app.nonexistent"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
}

#[test]
fn cache_honors_explicit_environment_and_custom_path() {
    let root = TestRoot::new();
    let base = &root.0;
    fs::write(base.join(".env"), "APP_NAME=Default\n").unwrap();
    fs::write(base.join(".env.preview"), "APP_NAME=Preview\n").unwrap();
    let custom = || {
        let mut command = application(base);
        command
            .arg("artisan")
            .env("APP_CONFIG_CACHE", "private/settings.json");
        command
    };
    success(
        custom()
            .args(["--env", "preview", "config:cache"])
            .output()
            .unwrap(),
    );
    assert!(base.join("private/settings.json").is_file());
    assert!(
        success(custom().args(["config:show", "app.name"]).output().unwrap()).contains("Preview")
    );
    assert!(success(artisan(base, &["config:show", "app.name"])).contains("Default"));
    success(custom().arg("config:clear").output().unwrap());
    assert!(!base.join("private/settings.json").exists());
}
