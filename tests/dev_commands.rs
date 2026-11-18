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
        .withProviders(vec![Box::new(DevelopmentProvider(false))])
        .create()
        .unwrap();
    let second = Application::configure(second.0.clone())
        .withProviders(vec![Box::new(DevelopmentProvider(true))])
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
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["command"], "./artisan serve");
    assert_eq!(
        list[1]["command"],
        "./artisan queue:listen --tries=1 --timeout=0"
    );
    assert_eq!(list[2]["command"], "yarn run dev");
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
