use openrust::{
    config::{ConfigError, Environment, Value},
    console::{Application as Artisan, Output},
    contracts::console::Kernel as _,
    foundation::{Application, configuration::Routing, console::Kernel, run_application},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::LogContext,
    routing::RoutingError,
    support::facades::{Log, Route},
};
use openrust_pail::{File, Options, PailServiceProvider, ProcessFactory, ProcessOutcome};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("pail-command-{:032x}", fastrand::u128(..)));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join(".env"), "APP_ENV=production\n").unwrap();
        Self(path)
    }
    fn sessions(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.0.join("storage/pail"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "pail"))
            .collect()
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .env("PAIL_COMMAND_FIXTURE", "1")
            .env("APP_BASE_PATH", &self.0)
            .env_remove("APP_ENV")
            .env_remove("VAPOR_SSM_PATH")
            .env_remove("PAIL_TESTS")
            .env_remove("APP_CONFIG_CACHE")
            .env_remove("APP_PACKAGES_CACHE")
            .env_remove("NO_COLOR")
            .env_remove("FORCE_COLOR")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn config(_: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":env.get_utf8("APP_ENV")?.unwrap_or("production"),"host":"127.0.0.1","port":0}),
        ),
        (
            "logging".into(),
            json!({"default":"single","channels":{"single":{"driver":"single","path":"original.log"}}}),
        ),
    ]))
}
fn routes() -> Result<(), RoutingError> {
    Route::get("/probe", probe)
}
fn probe(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Log::info("HTTP captured 日志", &LogContext::new()).unwrap();
        Ok(Response::new(StatusCode::OK, Body::from("ok")))
    })
}
fn application(root: PathBuf) -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Application::configure(root)
        .withConfig(config)
        .withProviders(vec![Box::new(PailServiceProvider)])
        .withRouting(Routing::default().web(routes))
        .create()?)
}
fn create() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    application(std::env::var_os("APP_BASE_PATH").unwrap().into())
}
async fn wait_sessions(root: &Root, count: usize) {
    for _ in 0..200 {
        if root.sessions().len() == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("session count did not reach {count}: {:?}", root.sessions());
}
async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let mut kernel = openrust::foundation::http::Kernel::new(application(root.0.clone()).unwrap());
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
    (
        address,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
async fn http(address: std::net::SocketAddr) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut data = vec![];
    stream.read_to_end(&mut data).await.unwrap();
    assert!(data.starts_with(b"HTTP/1.1 200"));
}
fn record(message: &str) -> String {
    format!(
        "{}\n",
        json!({"message":message,"context":{"__pail":{"origin":{"type":"console","command":"fixture"}}},"level_name":"INFO","datetime":"2026-11-19T12:00:00+00:00"})
    )
}
fn write_locked(path: &Path, bytes: &[u8], truncate: bool) {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(!cfg!(windows))
        .create(true)
        .open(path)
        .unwrap();
    // Windows enforces the reader's shared lock. Test producers use the same
    // exclusive-write contract as the real Pail handler, including partial lines.
    file.lock().unwrap();
    if truncate {
        file.set_len(0).unwrap();
    }
    file.seek(SeekFrom::End(0)).unwrap();
    file.write_all(bytes).unwrap();
    file.unlock().unwrap();
}
async fn direct_follow() {
    let root = Root::new();
    let path = root.0.join("follow.pail");
    std::fs::write(
        &path,
        (0..20)
            .map(|i| record(&format!("initial-{i:02}")))
            .collect::<String>(),
    )
    .unwrap();
    let token = CancellationToken::new();
    let writer_path = path.clone();
    let cancelled = token.clone();
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(180)).await;
        let line = record("split-中文");
        let split = line.find('中').unwrap() + 1;
        write_locked(&writer_path, &line.as_bytes()[..split], false);
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_locked(&writer_path, &line.as_bytes()[split..], false);
        tokio::time::sleep(Duration::from_millis(150)).await;
        write_locked(&writer_path, b"unfinished", false);
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_locked(&writer_path, b"", true);
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_locked(&writer_path, record("after-truncate").as_bytes(), false);
        tokio::time::sleep(Duration::from_millis(150)).await;
        std::fs::rename(&writer_path, writer_path.with_extension("old")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_locked(&writer_path, record("after-rotate").as_bytes(), false);
        tokio::time::sleep(Duration::from_millis(150)).await;
        cancelled.cancel();
    });
    let mut output = Output::default();
    let result = ProcessFactory::default()
        .with_shutdown(Some(token))
        .run(
            &File::new(path),
            &mut output,
            &root.0,
            &Options::new(0, None, None, None, None),
        )
        .await
        .unwrap();
    writer.await.unwrap();
    assert_eq!(result, ProcessOutcome::Interrupted);
    assert!(!output.stdout.contains("initial-09"));
    for value in [
        "initial-10",
        "initial-19",
        "split-中文",
        "after-truncate",
        "after-rotate",
    ] {
        assert!(
            output.stdout.contains(value),
            "missing {value}: {}",
            output.stdout
        );
    }
    println!("native follow: last ten, split UTF-8, truncate, rotate, cancellation passed");
}
async fn captured() {
    let root = Root::new();
    let app = application(root.0.clone()).unwrap();
    let artisan = Artisan::new();
    let mut kernel = Kernel::new(app, artisan);
    let mut output = Output::default();
    let args: Vec<OsString> = ["pail", "--timeout=0", "-vv", "--quiet"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(150),
            kernel.handle(&args, &mut output)
        )
        .await
        .is_err()
    );
    wait_sessions(&root, 0).await;
    assert!(!output.isVerbose());
    let args: Vec<OsString> = ["pail", "--timeout=1"]
        .into_iter()
        .map(Into::into)
        .collect();
    assert_eq!(kernel.handle(&args, &mut output).await, 0);
    assert!(output.stdout.contains("Maximum execution time exceeded."));
    wait_sessions(&root, 0).await;
    println!("programmatic cancellation: owned file and Output metadata restored; timeout passed");
}
async fn suite() {
    direct_follow().await;
    captured().await;
    let root = Root::new();
    let other = Root::new();
    let (address, server) = server(&root).await;
    let help = root
        .cli()
        .args(["artisan", "pail", "--help"])
        .output()
        .unwrap();
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    let text = String::from_utf8(help.stdout).unwrap();
    for option in [
        "--filter",
        "--message",
        "--level",
        "--auth",
        "--user",
        "--timeout",
    ] {
        assert!(text.contains(option));
    }
    let a = root
        .cli()
        .args([
            "artisan",
            "pail",
            "--timeout=2",
            "--level=info",
            "--message=captured",
            "-v",
        ])
        .spawn()
        .unwrap();
    let b = root
        .cli()
        .args(["artisan", "pail", "--timeout=2", "--message=absent"])
        .spawn()
        .unwrap();
    let c = other
        .cli()
        .args(["artisan", "pail", "--timeout=2"])
        .spawn()
        .unwrap();
    wait_sessions(&root, 2).await;
    wait_sessions(&other, 1).await;
    http(address).await;
    for (child, expected) in [(a, true), (b, false), (c, false)] {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert_eq!(text.contains("HTTP captured 日志"), expected, "{text}");
        assert!(text.contains("Maximum execution time exceeded."));
    }
    wait_sessions(&root, 0).await;
    wait_sessions(&other, 0).await;
    assert!(
        std::fs::read_to_string(root.0.join("storage/pail/.gitignore"))
            .unwrap()
            .contains("!.gitignore")
    );
    for args in [
        vec!["artisan", "pail", "--timeout=-1"],
        vec!["artisan", "pail", "--timeout=1", "--quiet"],
        vec!["artisan", "pail", "--timeout=1", "--silent"],
    ] {
        let negative = args.contains(&"--timeout=-1");
        let result = root.cli().args(args).output().unwrap();
        assert_eq!(result.status.code(), Some(i32::from(negative)));
        assert!(result.stdout.is_empty() || negative);
        wait_sessions(&root, 0).await;
    }
    let result = root
        .cli()
        .env("APP_ENV", "testing")
        .args(["artisan", "pail", "--timeout=1"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let result = root
        .cli()
        .env("APP_ENV", "testing")
        .env("PAIL_TESTS", "1")
        .args(["artisan", "pail", "--timeout=1"])
        .output()
        .unwrap();
    assert!(result.status.success());
    #[cfg(unix)]
    for signal in ["-INT", "-TERM"] {
        let child = root
            .cli()
            .args(["artisan", "pail", "--timeout=0"])
            .spawn()
            .unwrap();
        wait_sessions(&root, 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            Command::new("kill")
                .args([signal, &child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        wait_sessions(&root, 0).await;
    }
    let child = root
        .cli()
        .args(["artisan", "pail", "--timeout=5"])
        .spawn()
        .unwrap();
    wait_sessions(&root, 1).await;
    write_locked(&root.sessions()[0], b"not json\n", true);
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    wait_sessions(&root, 0).await;
    #[cfg(any(unix, windows))]
    {
        let mut child = root
            .cli()
            .args(["artisan", "pail", "--timeout=5"])
            .spawn()
            .unwrap();
        wait_sessions(&root, 1).await;
        drop(child.stdout.take());
        http(address).await;
        assert_eq!(child.wait_with_output().unwrap().status.code(), Some(1));
        wait_sessions(&root, 0).await;
    }
    server.abort();
    let _ = server.await;
    println!(
        "real HTTP / independent Artisan: Provider discovery, filters, multiple sessions, app isolation, timeout, quiet/silent, environment gates, signals, invalid JSON, broken pipe passed"
    );
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    if std::env::var_os("PAIL_COMMAND_FIXTURE").is_some() {
        return run_application(create).await;
    }
    tokio::time::timeout(Duration::from_secs(35), suite())
        .await
        .unwrap();
    std::process::ExitCode::SUCCESS
}
