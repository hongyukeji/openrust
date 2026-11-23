use std::{ffi::OsString, fs, path::PathBuf, process::Command, time::SystemTime};

use openrust::{
    console::{ConsoleFuture, Output},
    foundation::{Application, DevCommands},
    support::{ProviderResult, ServiceProvider, facades::Artisan},
};
use serde_json::Value;

#[path = "support/vendor/dev_registration.rs"]
mod package;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "openrust-dev-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct DevelopmentProvider(bool);

impl ServiceProvider for DevelopmentProvider {
    fn boot<'a>(&'a mut self, _: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            DevCommands::artisan("serve --port=9000", Some("server"))?.green();
            DevCommands::register("echo app queue", Some("queue"))?;
            package::register()?;
            DevCommands::nodeExec("tailwindcss --watch", Some("tailwind"))?;
            DevCommands::register("echo app probe", None)?;
            DevCommands::order(&["tailwind", "server"])?;
            DevCommands::except(&["echo"])?;
            if self.0 {
                DevCommands::withoutDefaultCommands()?;
                DevCommands::withoutVendorCommands()?;
                DevCommands::only(&["package", "tailwind", "vite"])?;
            }
            Ok(())
        })
    }
}

async fn list(app: &Application, arguments: &[&str]) -> (i32, Vec<Value>) {
    let args: Vec<_> = arguments.iter().map(OsString::from).collect();
    app.call(
        openrust::container::Callback::new(&[], |_| async {
            let mut output = Output::default();
            let status = Artisan::call("dev:list", &args, &mut output).await.unwrap();
            Ok((status, serde_json::from_str(&output.stdout).unwrap()))
        }),
        Default::default(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn provider_registration_is_isolated_and_shared_by_listing_filters() {
    let first = Fixture::new("first");
    let second = Fixture::new("second");
    fs::write(first.0.join("package.json"), "{}").unwrap();
    fs::write(first.0.join("pnpm-lock.yaml"), "").unwrap();
    fs::write(second.0.join("package.json"), "{}").unwrap();
    fs::write(second.0.join("bun.lock"), "").unwrap();
    let first = Application::configure(first.0.clone())
        .with_providers(vec![Box::new(DevelopmentProvider(false))])
        .create()
        .unwrap();
    let second = Application::configure(second.0.clone())
        .with_providers(vec![Box::new(DevelopmentProvider(true))])
        .create()
        .unwrap();
    let ((status, processes), (other_status, other)) =
        tokio::join!(list(&first, &[]), list(&second, &[]));
    assert_eq!(status, 0);
    let names: Vec<_> = processes
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["tailwind", "server", "queue", "vite", "package"]);
    assert_eq!(processes[0]["command"], "pnpm dlx tailwindcss --watch");
    assert_eq!(processes[1]["command"], "./artisan serve --port=9000");
    assert_eq!(processes[1]["color"], "#86efac");
    assert_eq!(processes[2]["command"], "echo app queue");
    assert_eq!(processes[3]["command"], "pnpm run dev");
    assert_eq!(processes[4]["priority"], 1);
    assert!(
        processes[4]["source"]
            .as_str()
            .unwrap()
            .contains("dev_registration.rs:")
    );
    assert_eq!(other_status, 0);
    assert_eq!(other.len(), 1);
    assert_eq!(other[0]["command"], "bunx tailwindcss --watch");
    assert_eq!(
        list(&first, &["--only-vendor"]).await,
        (0, vec![processes[4].clone()])
    );
    assert_eq!(
        list(&first, &["--filter=9000", "--except-vendor"]).await,
        (0, vec![processes[1].clone()])
    );
    assert_eq!(
        list(&first, &["--only-vendor", "--except-vendor"]).await,
        (1, vec![])
    );
    assert_eq!(list(&first, &["--filter=no-match"]).await, (1, vec![]));
}

#[test]
fn executable_lists_defaults_without_starting_services() {
    let fixture = Fixture::new("binary");
    fs::write(fixture.0.join("package.json"), "{}").unwrap();
    fs::write(fixture.0.join("yarn.lock"), "").unwrap();
    let run = |options: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "dev:list"])
            .args(options)
            .current_dir(&fixture.0)
            .env("APP_BASE_PATH", &fixture.0)
            .env("QUEUE_CONNECTION", "nonexistent")
            .output()
            .unwrap()
    };
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let list: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(list.len(), if cfg!(feature = "pail") { 4 } else { 3 });
    assert_eq!(list[0]["command"], "./artisan serve");
    assert_eq!(
        list[1]["command"],
        "./artisan queue:listen --tries=1 --timeout=0"
    );
    if cfg!(feature = "pail") {
        assert_eq!(list[2]["name"], "logs");
        assert_eq!(list[2]["command"], "./artisan pail --timeout=0");
        assert_eq!(list[2]["priority"], 0);
    }
    assert_eq!(list.last().unwrap()["command"], "yarn run dev");
    let output = run(&["--json", "--filter=queue"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<Vec<Value>>(&output.stdout).unwrap(),
        [list[1].clone()]
    );
    let output = run(&["--no-interaction", "--only-vendor"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"[]\n");
}

#[cfg(unix)]
#[test]
fn executable_dev_runs_http_and_restarts_a_delayed_failure() {
    use std::{
        io::{Read as _, Write as _},
        net::{TcpListener, TcpStream},
        os::unix::fs::PermissionsExt as _,
        process::Stdio,
        thread::{self, sleep},
        time::{Duration, Instant},
    };

    let fixture = Fixture::new("supervisor");
    fs::write(fixture.0.join(".env"), "APP_NAME=DevSupervisor\n").unwrap();
    let tools = fixture.0.join("bin");
    fs::create_dir(&tools).unwrap();
    fs::write(fixture.0.join("package.json"), "{}").unwrap();
    let npm = tools.join("npm");
    fs::write(
        &npm,
        r#"#!/bin/sh
if [ ! -f attempted ]; then
    touch attempted
    sleep 1.1
    exit 9
fi
printf '\033[31masset-ready\033[0m\n'
printf 'asset-error-stream\n' >&2
trap 'sleep 0.2; printf "asset-stopped\n"; exit 0' TERM
touch ready
while :; do sleep 1; done
"#,
    )
    .unwrap();
    fs::set_permissions(npm, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "dev", "--json", "--tabs"])
            .current_dir(&fixture.0)
            .env("APP_BASE_PATH", &fixture.0)
            .env("QUEUE_CONNECTION", "nonexistent")
            .env("SERVER_PORT", port.to_string())
            .env("PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };

    struct RunningChild(std::process::Child);
    impl Drop for RunningChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut child = RunningChild(run());
    let stdout = child.0.stdout.take().unwrap();
    let reader = thread::spawn(move || std::io::read_to_string(stdout).unwrap());
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut served = false;
    while !(served && fixture.0.join("ready").exists()) && Instant::now() < deadline {
        if let Ok(mut request) = TcpStream::connect(("127.0.0.1", port)) {
            request
                .set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            write!(
                request,
                "GET /up HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            served |= request.read_to_string(&mut response).is_ok()
                && response.starts_with("HTTP/1.1 200");
        }
        sleep(Duration::from_millis(30));
    }
    assert!(served, "HTTP development server did not respond");
    assert!(
        fixture.0.join("ready").exists(),
        "Vite process did not restart"
    );
    let pid = child.0.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "dev did not terminate after SIGTERM"
        );
        sleep(Duration::from_millis(25));
    };
    assert_eq!(status.code(), Some(1));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    let stream = reader.join().unwrap();
    let events: Vec<Value> = stream
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().all(|event| event["v"] == 1));
    assert_eq!(events.last().unwrap()["type"], "done");
    assert_eq!(events.last().unwrap()["code"], 1);
    let vite: Vec<_> = events
        .iter()
        .filter(|event| event["label"] == "vite")
        .collect();
    assert_eq!(
        vite.iter().filter(|event| event["type"] == "start").count(),
        2
    );
    assert!(
        vite.iter()
            .any(|event| event["type"] == "restarting" && event["attempt"] == 1)
    );
    assert!(
        vite.iter()
            .any(|event| event["stream"] == "stdout" && event["text"] == "asset-ready")
    );
    assert!(
        vite.iter()
            .any(|event| event["stream"] == "stderr" && event["text"] == "asset-error-stream")
    );
    assert!(vite.iter().any(|event| event["text"] == "asset-stopped"));
    assert!(
        events
            .iter()
            .any(|event| event["label"] == "queue" && event["reason"] == "crashed-immediately")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn dev_uses_registry_restart_and_output_settings() {
    struct InlineProvider;

    impl ServiceProvider for InlineProvider {
        fn boot<'a>(&'a mut self, _: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
            Box::pin(async move {
                DevCommands::withoutDefaultCommands()?;
                DevCommands::disableAutoRestart()?;
                DevCommands::with_timestamps()?;
                DevCommands::register(
                    "sleep 1.1; echo test-stdout; echo test-stderr >&2; exit 23",
                    Some("example"),
                )?;
                Ok(())
            })
        }
    }

    let fixture = Fixture::new("settings");
    let app = Application::configure(fixture.0.clone())
        .with_providers(vec![Box::new(InlineProvider)])
        .create()
        .unwrap();
    let (status, out) = app
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let mut output = Output::default();
                let status = Artisan::call("dev", &["--inline".into()], &mut output)
                    .await
                    .unwrap();
                Ok((status, output))
            }),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(status, 23);
    assert_eq!(out.stdout.lines().count(), 1);
    assert!(out.stdout.contains(" [example] test-stdout\n"));
    assert!(out.stderr.contains(" [example] test-stderr\n"));
    assert!(
        out.stderr
            .contains(" [example] Process exited with code 23 (restart-disabled)\n")
    );
}
