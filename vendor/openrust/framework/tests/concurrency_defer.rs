//! Exercises the public process defer API through a standalone Artisan entry.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use openrust::{
    concurrency::{Error, Registry, Task},
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
        run_application,
    },
    http::HttpFuture,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Concurrency, Context},
    },
};

struct ProbeProvider;

impl ServiceProvider for ProbeProvider {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = app.base_path().to_owned();
            let hook_root = root.clone();
            Context::dehydrating(move |copy| {
                assert_eq!(copy.get("stage"), Some(serde_json::json!("finished")));
                copy.add("dehydrated", serde_json::json!(true));
                std::fs::write(hook_root.join("dehydrating"), []).unwrap();
            })?;
            app.container()
                .make::<Registry>()?
                .register("probe", move |label: String| {
                    let root = root.clone();
                    async move {
                        assert_eq!(
                            Context::get("stage").unwrap(),
                            Some(serde_json::json!("finished"))
                        );
                        assert_eq!(
                            Context::getHidden("stage").unwrap(),
                            Some(serde_json::json!("private finished"))
                        );
                        assert_eq!(Context::get("old").unwrap(), None);
                        assert_eq!(
                            Context::get("dehydrated").unwrap(),
                            Some(serde_json::json!(true))
                        );
                        if label == "failure" {
                            return Err(std::io::Error::other("deferred probe failed"));
                        }
                        std::fs::write(root.join(format!("{label}.started")), [])?;
                        wait_for(&root.join(format!("{label}.release"))).await?;
                        std::fs::write(root.join(format!("{label}.done")), [])
                    }
                })?;
            Ok(())
        })
    }
}

struct ProbeCommand(CommandDefinition, i32);

impl Command for ProbeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let root = app.base_path();
            Context::add("stage", serde_json::json!("registered")).unwrap();
            Context::addHidden("stage", serde_json::json!("private registered")).unwrap();
            Context::add("old", serde_json::json!(true)).unwrap();
            if self.1 == 0 {
                let _ = Concurrency::defer([
                    Task::new("probe", "x".repeat(8 * 1024 * 1024)).unwrap(),
                    Task::new("probe", "failure").unwrap(),
                    Task::new("probe", "ordinary").unwrap(),
                ])
                .unwrap();
                assert!(!root.join("ordinary.started").exists());
            } else {
                let _ = Concurrency::defer([Task::new("probe", "skipped").unwrap()]).unwrap();
                let _ = Concurrency::defer([Task::new("probe", "always").unwrap()])
                    .unwrap()
                    .always();
                assert!(!root.join("always.started").exists());
            }
            assert!(!root.join("dehydrating").exists());
            Context::add("stage", serde_json::json!("finished")).unwrap();
            Context::addHidden("stage", serde_json::json!("private finished")).unwrap();
            Context::forget("old").unwrap();
            output.comment("command completed");
            eprintln!("command stderr");
            Ok(self.1)
        })
    }
}

fn register() -> Result<(), ConsoleError> {
    for (name, code) in [("probe:success", 0), ("probe:nonzero", 7)] {
        Artisan::add(Box::new(ProbeCommand(
            CommandDefinition::new(clap::Command::new(name)),
            code,
        )))?;
    }
    Ok(())
}

fn application() -> Result<Application, Box<dyn std::error::Error + Send + Sync>> {
    let root = PathBuf::from(std::env::var_os("APP_BASE_PATH").unwrap());
    Ok(Application::configure(root.clone())
        .withProviders(vec![Box::new(ProbeProvider)])
        .withRouting(Routing::default().commands(register))
        .withExceptions(move |exceptions| {
            exceptions.report::<Error>(move |error| {
                let report = if matches!(error, Error::PayloadTooLarge) {
                    "launch.report"
                } else {
                    assert!(error.to_string().contains("deferred probe failed"));
                    "failure.report"
                };
                std::fs::write(root.join(report), std::process::id().to_string()).unwrap();
                ReportDecision::Stop
            });
        })
        .create()?)
}

async fn wait_for(path: &Path) -> std::io::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other(format!("missing {}", path.display())))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "artisan")
    {
        return run_application(application).await;
    }
    let root = tempfile::Builder::new()
        .prefix("openrust defer ")
        .tempdir()
        .unwrap();
    for (command, label, code) in [
        ("probe:success", "ordinary", 0),
        ("probe:nonzero", "always", 7),
    ] {
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        let child = child
            .args(["artisan", command])
            .env("APP_BASE_PATH", root.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .expect("Artisan must return and close its pipes before the deferred task finishes")
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("command completed"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("command stderr"));
        let path = |suffix| root.path().join(format!("{label}.{suffix}"));
        wait_for(&path("started")).await.unwrap();
        assert!(!path("done").exists());
        std::fs::write(path("release"), []).unwrap();
        wait_for(&path("done")).await.unwrap();
        std::fs::remove_file(root.path().join("dehydrating")).unwrap();
        if code == 0 {
            let launch = root.path().join("launch.report");
            wait_for(&launch).await.unwrap();
            assert_eq!(
                std::fs::read_to_string(launch).unwrap().trim(),
                pid.to_string()
            );
            let report = root.path().join("failure.report");
            wait_for(&report).await.unwrap();
            assert_ne!(
                std::fs::read_to_string(report).unwrap().trim(),
                pid.to_string()
            );
        }
    }
    assert!(!root.path().join("skipped.started").exists());
    ExitCode::SUCCESS
}
