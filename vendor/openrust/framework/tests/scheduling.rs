use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use openrust::{
    console::ConsoleError,
    contracts::{foundation::Application as _, queue::JobHandler},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
    },
    queue::{JobContext, Payload, QueueFuture, QueueResult},
    support::facades::{Artisan, Queue, Schedule},
};
use serde_json::{Value, json};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

const MONDAY: u64 = 1_767_600_000; // 2026-01-05 08:00 UTC

struct ScheduledJob;
impl JobHandler for ScheduledJob {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            assert_eq!(context.queue(), "scheduled");
            assert_eq!(payload.data(), &json!({"id": 1}));
            println!("queued {}", payload.uuid());
            Ok(())
        })
    }
}

fn routes() -> Result<(), ConsoleError> {
    Artisan::command("fixture:hello", |output| output.comment("artisan command"))?;
    Queue::register("fixture.job", Arc::new(ScheduledJob))
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    Schedule::configure(|schedule| {
        let profile = std::env::var("SCHEDULING_PROFILE").unwrap();
        let at = std::env::var("SCHEDULING_TIME").unwrap().parse().unwrap();
        let start = std::time::Instant::now();
        let advances = profile == "work";
        schedule.useClock(move || {
            let fixed = UNIX_EPOCH + Duration::from_secs(at);
            if advances {
                fixed + start.elapsed()
            } else {
                fixed
            }
        });
        if profile == "basic" {
            schedule.command("fixture:hello", []).dailyAt("17:00")?;
            schedule
                .call(|_, output| {
                    Box::pin(async move {
                        output.comment("local timezone and cron day OR");
                        Ok(())
                    })
                })
                .cron("0 3 31 * MON")?
                .timezone(chrono_tz::America::New_York);
            schedule
                .call(|_, output| {
                    Box::pin(async move {
                        output.comment("calendar should not run");
                        Ok(())
                    })
                })
                .dailyAt("08:00")?;
            schedule
                .call(|_, output| {
                    Box::pin(async move {
                        output.comment("environment should not run");
                        Ok(())
                    })
                })
                .environments(&["production"]);
            schedule
                .call(|_, output| {
                    Box::pin(async move {
                        output.comment("condition should not run");
                        Ok(())
                    })
                })
                .skip(|_| true);
            schedule.exec(
                "/bin/sh",
                [
                    "-c".into(),
                    "printf '%s\\n' \"$1\"; exit 7".into(),
                    "sh".into(),
                    "literal argument".into(),
                ],
            );
            schedule
                .call(|_, _| Box::pin(async { Err(ConsoleError::new("scheduled callback error")) }))
                .name("failing callback");
            schedule.job(
                Payload::new("fixture.job", json!({"id":1})).unwrap(),
                Some("scheduled"),
                Some("sync"),
            );
            return Ok(());
        }

        let event = schedule
            .call(move |app, output| {
                let repeat = profile == "repeat";
                let fail = profile == "overlap";
                Box::pin(async move {
                    let root = app.base_path();
                    let receipt = root.join("receipts");
                    use tokio::io::AsyncWriteExt as _;
                    let mut file = tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(receipt)
                        .await
                        .map_err(console_error)?;
                    file.write_all(b"ran\n").await.map_err(console_error)?;
                    if !repeat {
                        timeout(Duration::from_secs(9), async {
                            while !root.join("release").exists() {
                                sleep(Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .map_err(console_error)?;
                    }
                    output.comment("lock winner");
                    if fail {
                        return Err(ConsoleError::new("overlap holder failed"));
                    }
                    Ok(())
                })
            })
            .name("shared")
            .when(|_| true);
        match std::env::var("SCHEDULING_PROFILE").unwrap().as_str() {
            "overlap" => {
                event.withoutOverlapping();
            }
            "work" | "election" => {
                event.onOneServer();
            }
            "repeat" => {
                event.everySecond().onOneServer();
            }
            _ => unreachable!(),
        }
        Ok(())
    })
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

#[tokio::test]
#[ignore = "Spawned by the scheduler integration tests"]
async fn scheduler_artisan_fixture() {
    let root = PathBuf::from(std::env::var_os("SCHEDULING_ROOT").unwrap());
    let arguments: Vec<String> =
        serde_json::from_str(&std::env::var("SCHEDULING_ARGS").unwrap()).unwrap();
    let arguments = arguments
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let app = Application::configure(root)
        .withConfig(|base, _| {
            let config: serde_json::Map<String, Value> =
                serde_json::from_slice(&std::fs::read(base.join("config.json")).unwrap()).unwrap();
            Ok(config.into_iter().collect())
        })
        .withRouting(Routing::default().commands(routes))
        .withExceptions(|exceptions| {
            exceptions.report::<ConsoleError>(|error| {
                if error.to_string().contains("scheduled callback error") {
                    println!("scheduled failure reported");
                }
                ReportDecision::Stop
            });
        })
        .create()
        .unwrap();
    assert_eq!(app.handleCommand(&arguments).await.unwrap(), 0);
}

fn root(port: Option<u16>) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("openrust-scheduling-{}-{id}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let config = json!({
        "app":{"env":"testing", "timezone":"UTC", "schedule_timezone":"Asia/Tokyo"},
        "database":{"redis":{"default":{"host":"127.0.0.1","port":port.unwrap_or(6379)}}},
        "cache":{"default":if port.is_some() {"redis"} else {"array"}, "prefix":format!("schedule:{id}:"),
                 "stores":{"redis":{"driver":"redis","connection":"default"},"array":{"driver":"array"}}},
        "queue":{"default":"sync","connections":{"sync":{"driver":"sync"}}},
        "logging":{"default":"stderr","channels":{"stderr":{"driver":"errorlog"}}}
    });
    std::fs::write(root.join("config.json"), config.to_string()).unwrap();
    root
}

fn artisan(root: &Path, profile: &str, at: u64, args: &[&str]) -> Command {
    let mut process = Command::new(std::env::current_exe().unwrap());
    process
        .args([
            "--exact",
            "scheduler_artisan_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("SCHEDULING_ROOT", root)
        .env("SCHEDULING_PROFILE", profile)
        .env("SCHEDULING_TIME", at.to_string())
        .env("SCHEDULING_ARGS", serde_json::to_string(args).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    process
}

async fn run(root: &Path, profile: &str, at: u64, args: &[&str]) -> Output {
    let output = timeout(
        Duration::from_secs(12),
        artisan(root, profile, at, args).output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_success(&output);
    output
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn artisan_runs_due_tasks_and_reports_failures_without_stopping_the_schedule() {
    let root = root(None);
    let first = run(&root, "basic", MONDAY, &["schedule:run"]).await;
    let second = run(&root, "basic", MONDAY, &["schedule:run"]).await;
    let stdout = String::from_utf8(first.stdout).unwrap();
    let stderr = String::from_utf8(first.stderr).unwrap();
    for expected in [
        "artisan command",
        "local timezone and cron day OR",
        "literal argument",
        "scheduled failure reported",
        "queued ",
    ] {
        assert!(stdout.contains(expected), "{expected}: {stdout}\n{stderr}");
    }
    assert!(!stdout.contains("should not run"));
    assert!(stderr.contains("status: 7"), "{stderr}");
    assert!(stderr.contains("scheduled callback error"), "{stderr}");
    let uuid = stdout
        .lines()
        .find_map(|line| line.strip_prefix("queued "))
        .unwrap();
    assert!(!String::from_utf8(second.stdout).unwrap().contains(uuid));
    let output = run(&root, "basic", MONDAY, &["schedule:list"]).await;
    let listing = String::from_utf8(output.stdout).unwrap();
    assert!(
        listing.contains("fixture:hello") && listing.contains("+09:00"),
        "{listing}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

async fn wait_for_receipts(root: &Path, count: usize) {
    timeout(Duration::from_secs(8), async {
        while std::fs::read_to_string(root.join("receipts"))
            .unwrap_or_default()
            .lines()
            .count()
            < count
        {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn artisan_instances_coordinate_overlap_election_and_interrupt_on_redis() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        assert!(
            std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none(),
            "Redis fixture is required"
        );
        return;
    };
    let port = port.parse().unwrap();
    for profile in ["overlap", "election"] {
        let root = root(Some(port));
        let winner = artisan(&root, profile, MONDAY, &["schedule:run"])
            .spawn()
            .unwrap();
        wait_for_receipts(&root, 1).await;
        let contender = run(&root, profile, MONDAY, &["schedule:run", "--whisper"]).await;
        let stdout = String::from_utf8(contender.stdout).unwrap();
        assert!(!stdout.contains("lock winner") && !stdout.contains("No scheduled commands"));
        std::fs::write(root.join("release"), "").unwrap();
        let winner = timeout(Duration::from_secs(5), winner.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert_success(&winner);

        let after = run(&root, profile, MONDAY, &["schedule:run"]).await;
        let stdout = String::from_utf8(after.stdout).unwrap();
        assert_eq!(stdout.contains("lock winner"), profile == "overlap");
        assert_eq!(
            stdout.contains("No scheduled commands"),
            profile == "election"
        );
        run(&root, profile, MONDAY + 60, &["schedule:run"]).await;
        let count = if profile == "overlap" { 3 } else { 2 };
        assert_eq!(
            std::fs::read_to_string(root.join("receipts"))
                .unwrap()
                .lines()
                .count(),
            count
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    let root = root(Some(port));
    let winner = artisan(&root, "repeat", MONDAY + 54, &["schedule:run"])
        .spawn()
        .unwrap();
    wait_for_receipts(&root, 2).await;
    run(&root, "repeat", MONDAY + 54, &["schedule:interrupt"]).await;
    assert_success(
        &timeout(Duration::from_secs(3), winner.wait_with_output())
            .await
            .unwrap()
            .unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(root.join("receipts"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn artisan_schedule_work_finishes_the_current_task_on_sigterm() {
    let root = root(None);
    let worker = artisan(&root, "work", MONDAY + 59, &["schedule:work"])
        .spawn()
        .unwrap();
    wait_for_receipts(&root, 1).await;
    assert!(
        Command::new("kill")
            .args(["-TERM", &worker.id().unwrap().to_string()])
            .status()
            .await
            .unwrap()
            .success()
    );
    std::fs::write(root.join("release"), "").unwrap();
    let output = timeout(Duration::from_secs(4), worker.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_success(&output);
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("lock winner")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("receipts")).unwrap(),
        "ran\n"
    );
    std::fs::remove_dir_all(root).unwrap();
}
