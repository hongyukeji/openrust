use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use openrust::testing::TestApp;

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
    command
        .env("APP_BASE_PATH", root)
        .env("APP_DEBUG", "false")
        .env("APP_KEY", "01234567890123456789012345678901")
        .env("SESSION_DRIVER", "array");
    command
}

fn artisan(root: &Path, args: &[&str]) -> Output {
    command(root).arg("artisan").args(args).output().unwrap()
}

fn success(root: &Path, args: &[&str]) -> String {
    let output = artisan(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

struct Server {
    child: Child,
    address: SocketAddr,
}

impl Server {
    fn start(root: &Path, config: &[(&str, &str)]) -> Self {
        let address = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let child = command(root)
            .envs(config.iter().copied())
            .env("APP_HOST", "127.0.0.1")
            .env("APP_PORT", address.port().to_string())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut server = Self { child, address };
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect_timeout(&server.address, Duration::from_millis(50)).is_err() {
            assert!(server.child.try_wait().unwrap().is_none(), "server exited");
            assert!(Instant::now() < deadline, "server did not listen");
            thread::sleep(Duration::from_millis(20));
        }
        server
    }

    fn request(&self, method: &str, path: &str, accept: &str, cookie: Option<&str>) -> Reply {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: {accept}\r\nConnection: close\r\n",
            self.address
        )
        .unwrap();
        if let Some(cookie) = cookie {
            write!(stream, "Cookie: {cookie}\r\n").unwrap();
        }
        write!(stream, "\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        Reply {
            headers: headers.into(),
            body: body.into(),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Reply {
    headers: String,
    body: String,
}

impl Reply {
    fn status(&self) -> u16 {
        self.headers
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then(|| value.trim()))
    }
}

#[test]
fn artisan_maintenance_changes_live_http_and_isolates_application_roots() {
    let first = TestApp::create(|app| app).unwrap();
    let second = TestApp::create(|app| app).unwrap();
    let root = first.root();
    let views = root.join("resources/views");
    fs::create_dir_all(&views).unwrap();
    let view = views.join("offline.blade.html");
    fs::write(
        &view,
        "<p>Scheduled maintenance, retry {{ $retryAfter }}</p>",
    )
    .unwrap();

    let file = [("APP_MAINTENANCE_DRIVER", "file")];
    let live = Server::start(root, &file);
    let other = Server::start(second.root(), &file);
    let get = |path, accept, cookie| live.request("GET", path, accept, cookie);
    assert_eq!(get("/up", "text/html", None).status(), 200);
    assert!(success(root, &["up"]).contains("already up"));
    assert!(success(root, &["down"]).contains("now in maintenance"));
    assert_eq!(get("/up", "text/html", None).status(), 503);
    assert_eq!(other.request("GET", "/up", "text/html", None).status(), 200);

    let failed = artisan(root, &["down", "--render=missing-view"]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Failed to enter maintenance mode"));
    assert_eq!(get("/up", "text/html", None).status(), 503);

    assert!(
        success(
            root,
            &[
                "down",
                "--render=offline",
                "--redirect=unavailable",
                "--retry=60",
                "--refresh=15",
                "--status=502",
                "--secret=maintenance-test-bypass",
            ]
        )
        .contains("options updated")
    );
    let redirect = get("/up", "text/html", None);
    assert_eq!(redirect.status(), 302);
    assert!(
        redirect
            .header("location")
            .unwrap()
            .ends_with("/unavailable")
    );
    fs::write(&view, "Changed source").unwrap();
    let html = get("/unavailable", "text/html", None);
    assert_eq!(html.status(), 502);
    assert!(
        html.body.contains("Scheduled maintenance, retry 60"),
        "{}",
        html.body
    );
    assert_eq!(html.header("retry-after"), Some("60"));
    assert_eq!(html.header("refresh"), Some("15"));
    let json = get("/up", "application/json", None);
    assert_eq!(json.status(), 502);
    assert_eq!(json.header("content-type"), Some("application/json"));
    assert_eq!(json.header("retry-after"), Some("60"));
    assert!(json.body.contains("Service Unavailable"), "{}", json.body);
    assert!(
        live.request("HEAD", "/unavailable", "text/html", None)
            .body
            .is_empty()
    );

    let bypass = get("/maintenance-test-bypass", "text/html", None);
    assert_eq!(bypass.status(), 302);
    assert_eq!(
        bypass.header("location").unwrap().trim_end_matches('/'),
        format!("http://{}", live.address)
    );
    let cookie = bypass.header("set-cookie").unwrap();
    assert!(cookie.to_ascii_lowercase().contains("httponly"));
    assert_eq!(bypass.header("cache-control"), Some("no-store, private"));
    let cookie = cookie.split(';').next().unwrap();
    assert!(cookie.starts_with("laravel_maintenance="));
    assert_eq!(get("/up", "text/html", Some(cookie)).status(), 200);
    assert_eq!(
        get(
            "/up",
            "application/json",
            Some("laravel_maintenance=invalid")
        )
        .status(),
        502
    );

    success(
        root,
        &["down", "--with-secret", "--retry=2028-06-05T14:30:00Z"],
    );
    let state: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("storage/framework/down")).unwrap()).unwrap();
    let generated = state["secret"].as_str().unwrap();
    assert_eq!(generated.len(), 32);
    assert_ne!(generated, "maintenance-test-bypass");
    let rejected = get("/up", "application/json", Some(cookie));
    assert_eq!(rejected.status(), 503);
    assert_eq!(
        rejected.header("retry-after"),
        Some("Mon, 05 Jun 2028 14:30:00 GMT")
    );
    assert!(success(root, &["up"]).contains("now live"));
    assert_eq!(get("/up", "text/html", None).status(), 200);
    assert!(success(root, &["up"]).contains("already up"));
}

#[test]
fn artisan_maintenance_cache_is_shared_by_http_instances() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        return;
    };
    let first = TestApp::create(|app| app).unwrap();
    let second = TestApp::create(|app| app).unwrap();
    let prefix = format!(
        "maintenance_http_{}",
        first.root().file_name().unwrap().to_string_lossy()
    );
    let settings = [
        ("APP_MAINTENANCE_DRIVER", "cache"),
        ("APP_MAINTENANCE_STORE", "redis"),
        ("CACHE_PREFIX", prefix.as_str()),
        ("REDIS_HOST", "127.0.0.1"),
        ("REDIS_PORT", port.as_str()),
    ];
    let first_server = Server::start(first.root(), &settings);
    let second_server = Server::start(second.root(), &settings);
    let run = |root: &Path, action| {
        let output = command(root)
            .args(["artisan", action])
            .envs(settings)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(first.root(), "down");
    let before = [&first_server, &second_server].map(|server| {
        server
            .request("GET", "/up", "application/json", None)
            .status()
    });
    run(second.root(), "up");
    assert_eq!(before, [503, 503]);
    for server in [&first_server, &second_server] {
        assert_eq!(
            server
                .request("GET", "/up", "application/json", None)
                .status(),
            200
        );
    }
    assert!(!first.root().join("storage/framework/down").exists());
    assert!(!second.root().join("storage/framework/down").exists());
}
