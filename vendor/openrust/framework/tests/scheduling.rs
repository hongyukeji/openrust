use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

use openrust::{
    console::{
        ConsoleError, ConsoleFuture, Output as ConsoleOutput,
        events::{
            SchedulePaused, ScheduleResumed, ScheduledTaskFailed, ScheduledTaskFinished,
            ScheduledTaskSkipped, ScheduledTaskStarting,
        },
    },
    contracts::{
        events::{Event as EventPayload, Listener},
        foundation::Application as _,
        queue::JobHandler,
    },
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    foundation::{
        Application,
        configuration::{ReportDecision, Routing},
    },
    queue::{JobContext, Payload, QueueFuture, QueueResult},
    support::facades::{Artisan, Event, Queue, Schedule},
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

struct PauseEvents;

impl Listener for PauseEvents {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(skipped) = event.downcast_ref::<ScheduledTaskSkipped>() {
                println!("skipped {}", skipped.task.getSummaryForDisplay());
            } else {
                assert!(
                    event.downcast_ref::<SchedulePaused>().is_some()
                        || event.downcast_ref::<ScheduleResumed>().is_some()
                );
                println!("event {}", event.name());
            }
            Ok(None)
        })
    }
}

#[derive(Default)]
struct ScheduleEvents {
    task: Mutex<Option<Arc<openrust::console::scheduling::Event>>>,
}

impl Listener for ScheduleEvents {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventPayload,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if let Some(starting) = event.downcast_ref::<ScheduledTaskStarting>() {
                *self.task.lock().unwrap() = Some(starting.task.clone());
                println!("starting {}", starting.task.getSummaryForDisplay());
            } else if let Some(finished) = event.downcast_ref::<ScheduledTaskFinished>() {
                assert!(Arc::ptr_eq(
                    self.task.lock().unwrap().as_ref().unwrap(),
                    &finished.task
                ));
                assert!(finished.runtime < Duration::from_secs(10));
                let task = finished.task.getSummaryForDisplay();
                println!("finished {task}");
                if task == "listener-error" {
                    return Err(EventError::Listener {
                        source: Box::new(ConsoleError::new("finished listener error")),
                    });
                }
            } else if let Some(failed) = event.downcast_ref::<ScheduledTaskFailed>() {
                assert!(Arc::ptr_eq(
                    self.task.lock().unwrap().as_ref().unwrap(),
                    &failed.task
                ));
                assert!(failed.runtime < Duration::from_secs(10));
                if failed.task.getSummaryForDisplay() == "listener-error" {
                    assert_eq!(
                        failed.exception.source().unwrap().to_string(),
                        "finished listener error"
                    );
                }
                println!(
                    "failed {}: {}",
                    failed.task.getSummaryForDisplay(),
                    failed.exception
                );
            } else if let Some(skipped) = event.downcast_ref::<ScheduledTaskSkipped>() {
                println!("skipped {}", skipped.task.getSummaryForDisplay());
            }
            Ok(None)
        })
    }
}

fn routes() -> Result<(), ConsoleError> {
    Artisan::command("fixture:hello", |output| output.comment("artisan command"))?;
    Artisan::command("inspire", |output| {
        output.comment("maintenance example ran")
    })?;
    Queue::register("fixture.job", Arc::new(ScheduledJob))
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let profile = std::env::var("SCHEDULING_PROFILE").unwrap();
    if profile.starts_with("pause") || matches!(profile.as_str(), "basic" | "election" | "overlap")
    {
        for name in [
            SchedulePaused.name(),
            ScheduleResumed.name(),
            "console.scheduled_task_skipped",
        ] {
            Event::listen(name, Arc::new(PauseEvents), Delivery::Sync).map_err(console_error)?;
        }
    }
    if matches!(profile.as_str(), "events" | "hooks") {
        let listener = Arc::new(ScheduleEvents::default());
        for name in [
            "console.scheduled_task_starting",
            "console.scheduled_task_finished",
            "console.scheduled_task_failed",
            "console.scheduled_task_skipped",
        ] {
            Event::listen(name, listener.clone(), Delivery::Sync).map_err(console_error)?;
        }
    }
    Schedule::configure(|schedule| {
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
        if profile == "output-mail" {
            schedule
                .command("fixture:hello", [])
                .name("Scheduled success")
                .emailOutputTo(["success@example.test"]);
            schedule
                .command("fixture:missing", [])
                .name("Scheduled failure")
                .emailOutputOnFailure(["failure@example.test"]);
            schedule
                .command("fixture:hello", [])
                .emailOutputOnFailure(["unexpected@example.test"]);
            return Ok(());
        }
        if profile == "output" {
            schedule
                .command("fixture:hello", [])
                .sendOutputTo("overwrite.log");
            schedule
                .command("fixture:hello", [])
                .appendOutputTo("append.log");
            schedule
                .command("fixture:missing", [])
                .sendOutputTo("failure.log")
                .onFailureWithOutput(
                    |app, _, bytes| {
                        Box::pin(async move {
                            std::fs::write(app.base_path().join("failure-hook.log"), bytes)
                                .map_err(console_error)
                        })
                    },
                    true,
                );
            return Ok(());
        }
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
                .name("filtered task")
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

        if profile == "hooks" {
            schedule
                .before(hook("group-before"))
                .after(hook("group-after"))
                .onSuccess(hook("group-success"))
                .onFailure(hook("group-failure"))
                .group(|schedule| {
                    schedule.before(hook("nested-before")).group(|schedule| {
                        schedule
                            .command("fixture:hello", [])
                            .name("hook-success")
                            .before(hook("task-before"))
                            .then(hook("task-then"));
                        Ok(())
                    })?;
                    schedule.command("fixture:missing", []).name("hook-nonzero");
                    schedule
                        .call(|_, _| {
                            Box::pin(async { Err(ConsoleError::new("scheduled callback error")) })
                        })
                        .name("hook-callback-error");
                    Ok(())
                })?;

            group_task(schedule, "before-error")
                .before(|_, _| Box::pin(async { Err(ConsoleError::new("before hook error")) }))
                .after(hook("unreachable-after"));
            group_task(schedule, "after-error")
                .after(|_, _| Box::pin(async { Err(ConsoleError::new("after hook error")) }))
                .then(hook("unreachable-then"));
            group_task(schedule, "outside");
            return Ok(());
        }

        if matches!(profile.as_str(), "events" | "events-isolated") {
            schedule.command("fixture:hello", []).name("success");
            schedule.command("fixture:missing", []).name("nonzero");
            schedule
                .call(|_, _| Box::pin(async { Err(ConsoleError::new("scheduled callback error")) }))
                .name("exception");
            schedule
                .exec("openrust-test-executable-that-does-not-exist", [])
                .name("spawn-error");
            group_task(schedule, "filtered").skip(|_| true);
            group_task(schedule, "listener-error");
            group_task(schedule, "last");
            return Ok(());
        }

        if profile == "groups" {
            assert!(schedule.group(|_| Ok(())).is_err());
            assert!(schedule.cron("bad").is_err());
            schedule
                .timezone(chrono_tz::Asia::Tokyo)
                .dailyAt("17:00")?
                .group(|schedule| {
                    schedule.command("inspire", []);
                    Ok(())
                })?;

            schedule
                .dailyAt("17:00")?
                .timezone(chrono_tz::Asia::Tokyo)
                .environments(&["testing"])
                .evenInMaintenanceMode()
                .evenWhenPaused()
                .withAttributes([("team".into(), json!("outer"))])
                .group(|schedule| {
                    group_task(schedule, "inherited");
                    schedule
                        .timezone(chrono_tz::UTC)
                        .withAttributes([("team".into(), json!("inner"))])
                        .group(|schedule| {
                            group_task(schedule, "nested-not-due");
                            let event = group_task(schedule, "nested-due").dailyAt("08:00")?;
                            assert_eq!(event.attributes()["team"], json!(["outer", "inner"]));
                            Ok(())
                        })?;
                    schedule.skip(|_| true);
                    group_task(schedule, "filtered");
                    schedule.dailyAt("18:00")?;
                    group_task(schedule, "one-shot");
                    let sibling = group_task(schedule, "sibling");
                    assert_eq!(sibling.attributes()["team"], "outer");
                    assert!(
                        schedule
                            .timezone(chrono_tz::UTC)
                            .group(|_| Err(ConsoleError::new("group callback failed")))
                            .is_err()
                    );
                    group_task(schedule, "after-error");
                    Ok(())
                })?;
            assert!(group_task(schedule, "outside").attributes().is_empty());
            return Ok(());
        }

        if profile == "groups-repeat" {
            schedule.everySecond().group(|schedule| {
                group_task(schedule, "repeated");
                group_task(schedule, "once").everyMinute();
                Ok(())
            })?;
            return Ok(());
        }

        let pause = profile.starts_with("pause");
        if pause || matches!(profile.as_str(), "maintenance" | "maintenance-repeat") {
            if pause {
                schedule.useCache("array");
                if profile == "pause-disabled" {
                    schedule.doNotAllowPausing();
                }
                if profile != "pause-repeat" {
                    schedule.command("inspire", []).evenWhenPaused();
                }
            }
            if profile == "maintenance" {
                schedule.command("inspire", []).evenInMaintenanceMode();
            }
            for (name, exempt) in [("ordinary", false), ("exempt", true)] {
                let waits = profile == "pause-repeat" && !exempt;
                let event = schedule.call(move |app, output| {
                    Box::pin(async move {
                        let mut receipt = tokio::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(app.base_path().join(name))
                            .await
                            .map_err(console_error)?;
                        use tokio::io::AsyncWriteExt as _;
                        receipt.write_all(b"ran\n").await.map_err(console_error)?;
                        if waits {
                            timeout(Duration::from_secs(4), async {
                                while !app.base_path().join("release").exists() {
                                    sleep(Duration::from_millis(10)).await;
                                }
                            })
                            .await
                            .map_err(console_error)?;
                        }
                        output.comment(name);
                        Ok(())
                    })
                });
                event.name(name);
                if exempt {
                    if pause {
                        event.evenWhenPaused();
                    } else {
                        event.evenInMaintenanceMode();
                    }
                }
                if profile.ends_with("-repeat") {
                    event.everySecond();
                }
            }
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

fn hook(
    label: &'static str,
) -> impl for<'a> Fn(&'a Application, &'a mut ConsoleOutput) -> ConsoleFuture<'a, Result<(), ConsoleError>>
{
    move |_, output| {
        // Emit immediately so the fixture can compare hooks and synchronous events.
        Box::pin(async move {
            println!("hook {label}");
            output.comment(label);
            Ok(())
        })
    }
}

fn group_task<'a>(
    schedule: &'a mut openrust::console::scheduling::Schedule,
    name: &'static str,
) -> &'a mut openrust::console::scheduling::CallbackEvent {
    let task = schedule.call(move |_, output| {
        Box::pin(async move {
            output.comment(name);
            Ok(())
        })
    });
    task.name(name);
    task
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
    let status = app.handleCommand(&arguments).await.unwrap();
    if status != 0 {
        std::process::exit(status);
    }
}

fn root(port: Option<u16>) -> PathBuf {
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let id = format!("{}-{timestamp}-{sequence}", std::process::id());
    let root = std::env::temp_dir().join(format!("openrust-scheduling-{id}"));
    std::fs::create_dir(&root).unwrap();
    let config = json!({
        "app":{"env":"testing", "timezone":"UTC", "schedule_timezone":"Asia/Tokyo"},
        "database":{"redis":{"default":{"host":"127.0.0.1","port":port.unwrap_or(6379)}}},
        "cache":{"default":if port.is_some() {"redis"} else {"array"}, "prefix":format!("schedule:{id}:"),
                 "stores":{"redis":{"driver":"redis","connection":"default"},"array":{"driver":"array"}}},
        "mail":{"default":"log","from":{"address":"scheduler@example.test"},"mailers":{"log":{"transport":"log"}}},
        "queue":{"default":"sync","connections":{"sync":{"driver":"sync"}}},
        "logging":{"default":"stderr","channels":{"stderr":{"driver":"stderr"}}}
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

struct RunningScheduler {
    child: tokio::process::Child,
    stdout: File,
    stderr: File,
}

impl RunningScheduler {
    fn start(mut command: Command) -> Self {
        // Use files so waiting for this process never depends on another
        // concurrently launched fixture closing a pipe.
        let stdout = tempfile::tempfile().unwrap();
        let stderr = tempfile::tempfile().unwrap();
        let child = command
            .stdout(stdout.try_clone().unwrap())
            .stderr(stderr.try_clone().unwrap())
            .spawn()
            .unwrap();
        Self {
            child,
            stdout,
            stderr,
        }
    }

    async fn wait_with_output(mut self) -> io::Result<Output> {
        let status = self.child.wait().await?;
        let read = |mut file: File| -> io::Result<Vec<u8>> {
            file.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        Ok(Output {
            status,
            stdout: read(self.stdout)?,
            stderr: read(self.stderr)?,
        })
    }
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
    assert!(stdout.contains("skipped filtered task"));
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
        let winner = RunningScheduler::start(artisan(&root, profile, MONDAY, &["schedule:run"]));
        wait_for_receipts(&root, 1).await;
        let contender = run(&root, profile, MONDAY, &["schedule:run", "--whisper"]).await;
        let stdout = String::from_utf8(contender.stdout).unwrap();
        assert!(!stdout.contains("lock winner") && !stdout.contains("No scheduled commands"));
        assert!(stdout.contains("skipped shared"), "{stdout}");
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
    let winner = RunningScheduler::start(artisan(&root, "repeat", MONDAY + 54, &["schedule:run"]));
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
    let worker = RunningScheduler::start(artisan(&root, "work", MONDAY + 59, &["schedule:work"]));
    wait_for_receipts(&root, 1).await;
    assert!(
        Command::new("kill")
            .args(["-TERM", &worker.child.id().unwrap().to_string()])
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

fn pause_root(port: Option<u16>) -> PathBuf {
    let root = root(port);
    if port.is_none() {
        let config_path = root.join("config.json");
        let mut config: Value =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        config["cache"]["default"] = json!("file");
        config["cache"]["stores"]["file"] = json!({"driver":"file","path":"scheduler-cache"});
        std::fs::write(config_path, config.to_string()).unwrap();
    }
    root
}

#[tokio::test]
async fn artisan_pauses_and_resumes_scheduler_with_typed_events_and_isolation() {
    let redis = std::env::var("OPENRUST_TEST_REDIS_PORT")
        .ok()
        .map(|port| port.parse().unwrap());
    for port in std::iter::once(None).chain(redis.map(Some)) {
        let active = pause_root(port);
        let other = pause_root(port);
        let pause = run(&active, "pause", MONDAY, &["schedule:pause"]).await;
        assert!(String::from_utf8_lossy(&pause.stdout).contains("event console.schedule_paused"));
        let list = run(&active, "pause", MONDAY, &["schedule:list"]).await;
        assert!(String::from_utf8_lossy(&list.stdout).contains("ordinary"));

        let (paused, _) = tokio::join!(
            run(&active, "pause", MONDAY, &["schedule:run"]),
            run(&other, "pause", MONDAY, &["schedule:run"]),
        );
        let stdout = String::from_utf8_lossy(&paused.stdout);
        assert!(stdout.contains("skipped ordinary"), "{stdout}");
        assert!(stdout.contains("maintenance example ran"), "{stdout}");
        assert_eq!(maintenance_receipts(&active, "ordinary"), 0);
        assert_eq!(maintenance_receipts(&active, "exempt"), 1);
        assert_eq!(maintenance_receipts(&other, "ordinary"), 1);

        run(&active, "pause", MONDAY, &["down"]).await;
        let maintenance = run(&active, "pause", MONDAY, &["schedule:run"]).await;
        assert!(String::from_utf8_lossy(&maintenance.stdout).contains("No scheduled commands"));
        assert_eq!(maintenance_receipts(&active, "exempt"), 1);
        run(&active, "pause", MONDAY, &["up"]).await;

        let refused = artisan(&active, "pause-disabled", MONDAY, &["schedule:pause"])
            .output()
            .await
            .unwrap();
        assert_eq!(refused.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&refused.stderr).contains("pausing is currently disabled"));
        assert!(
            !String::from_utf8_lossy(&refused.stdout).contains("event console.schedule_paused")
        );
        run(&active, "pause-disabled", MONDAY, &["schedule:run"]).await;
        assert_eq!(maintenance_receipts(&active, "ordinary"), 1);

        for (command, profile) in [
            ("schedule:continue", "pause-disabled"),
            ("schedule:resume", "pause"),
        ] {
            let resume = run(&active, profile, MONDAY, &[command]).await;
            assert!(
                String::from_utf8_lossy(&resume.stdout).contains("event console.schedule_resumed")
            );
            run(&active, "pause", MONDAY, &["schedule:run"]).await;
            run(&active, "pause", MONDAY, &["schedule:pause"]).await;
        }
        assert_eq!(maintenance_receipts(&active, "ordinary"), 3);
        run(&active, "pause", MONDAY, &["schedule:resume"]).await;
        std::fs::remove_dir_all(active).unwrap();
        std::fs::remove_dir_all(other).unwrap();
    }
}

#[tokio::test]
async fn artisan_sub_minute_scheduler_observes_pause_and_resume_in_same_run() {
    let root = pause_root(None);
    let at = MONDAY + 53;
    let profile = "pause-repeat";
    let runner = RunningScheduler::start(artisan(&root, profile, at, &["schedule:run"]));
    let wait_for = |name, count| {
        let root = &root;
        async move {
            timeout(Duration::from_secs(4), async {
                while maintenance_receipts(root, name) < count {
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
    };
    wait_for("ordinary", 1).await;
    run(&root, profile, at, &["schedule:pause"]).await;
    assert_eq!(maintenance_receipts(&root, "exempt"), 0);
    std::fs::write(root.join("release"), "").unwrap();
    wait_for("exempt", 2).await;
    assert_eq!(maintenance_receipts(&root, "ordinary"), 1);
    run(&root, profile, at, &["schedule:resume"]).await;
    wait_for("ordinary", 2).await;
    let output = timeout(Duration::from_secs(8), runner.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("skipped ordinary"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_pause_reports_cache_errors_without_emitting_success_events() {
    let root = pause_root(None);
    let config_path = root.join("config.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["cache"]["default"] = json!("unconfigured");
    std::fs::write(config_path, config.to_string()).unwrap();
    for command in ["schedule:pause", "schedule:resume"] {
        let output = artisan(&root, "pause", MONDAY, &[command])
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unconfigured"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("event console.schedule_"));
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn maintenance_receipts(root: &Path, name: &str) -> usize {
    std::fs::read_to_string(root.join(name))
        .unwrap_or_default()
        .lines()
        .count()
}

#[tokio::test]
async fn artisan_maintenance_skips_ordinary_tasks_and_keeps_apps_isolated() {
    let redis = std::env::var("OPENRUST_TEST_REDIS_PORT")
        .ok()
        .map(|port| port.parse().unwrap());
    for port in std::iter::once(None).chain(redis.map(Some)) {
        let active = root(port);
        let other = root(port);
        if port.is_some() {
            for base in [&active, &other] {
                let path = base.join("config.json");
                let mut config: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                config["app"]["maintenance"] = json!({"driver":"cache","store":"redis"});
                std::fs::write(path, config.to_string()).unwrap();
            }
        }
        run(&active, "maintenance", MONDAY, &["down"]).await;
        let (down, _) = tokio::join!(
            run(&active, "maintenance", MONDAY, &["schedule:run"]),
            run(&other, "maintenance", MONDAY, &["schedule:run"]),
        );
        let stdout = String::from_utf8_lossy(&down.stdout);
        assert!(stdout.contains("maintenance example ran"));
        assert!(!stdout.contains("No scheduled commands"));
        assert_eq!(maintenance_receipts(&active, "ordinary"), 0);
        assert_eq!(maintenance_receipts(&active, "exempt"), 1);
        assert_eq!(maintenance_receipts(&other, "ordinary"), 1);
        assert_eq!(maintenance_receipts(&other, "exempt"), 1);
        run(&active, "maintenance", MONDAY, &["up"]).await;
        run(&active, "maintenance", MONDAY, &["schedule:run"]).await;
        assert_eq!(maintenance_receipts(&active, "ordinary"), 1);
        assert_eq!(maintenance_receipts(&active, "exempt"), 2);
        std::fs::remove_dir_all(active).unwrap();
        std::fs::remove_dir_all(other).unwrap();
    }
}

#[tokio::test]
async fn artisan_sub_minute_tasks_remember_maintenance_until_the_next_run() {
    let root = root(None);
    let at = MONDAY + 53;
    let profile = "maintenance-repeat";
    let runner = RunningScheduler::start(artisan(&root, profile, at, &["schedule:run"]));
    let wait_for_exempt = |count| {
        let root = &root;
        async move {
            timeout(Duration::from_secs(4), async {
                while maintenance_receipts(root, "exempt") < count {
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
    };
    wait_for_exempt(1).await;
    run(&root, profile, at, &["down"]).await;
    wait_for_exempt(2).await;
    assert_eq!(maintenance_receipts(&root, "ordinary"), 1);
    run(&root, profile, at, &["up"]).await;
    wait_for_exempt(3).await;
    assert_eq!(maintenance_receipts(&root, "ordinary"), 1);
    assert_success(
        &timeout(Duration::from_secs(8), runner.wait_with_output())
            .await
            .unwrap()
            .unwrap(),
    );
    assert_eq!(maintenance_receipts(&root, "ordinary"), 1);
    run(&root, "maintenance", MONDAY + 60, &["schedule:run"]).await;
    assert_eq!(maintenance_receipts(&root, "ordinary"), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_schedule_groups_inherit_and_restore_attributes() {
    let root = pause_root(None);
    let profile = "groups";
    let run_output = run(&root, profile, MONDAY, &["schedule:run"]).await;
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("maintenance example ran"), "{stdout}");
    assert!(stdout.contains("outside"), "{stdout}");

    run(&root, profile, MONDAY, &["down"]).await;
    run(&root, profile, MONDAY, &["schedule:pause"]).await;
    let run_output = run(&root, profile, MONDAY, &["schedule:run"]).await;
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    for name in ["inherited", "nested-due", "sibling", "after-error"] {
        assert!(
            stdout.lines().any(|line| line.trim() == name),
            "{name}: {stdout}"
        );
    }
    for name in [
        "nested-not-due",
        "filtered",
        "one-shot",
        "outside",
        "maintenance example ran",
    ] {
        assert!(
            !stdout.lines().any(|line| line.trim() == name),
            "{name}: {stdout}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_schedule_groups_inherit_sub_minute_frequency_with_task_override() {
    let root = root(None);
    let run_output = run(&root, "groups-repeat", MONDAY + 56, &["schedule:run"]).await;
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(
        stdout
            .lines()
            .filter(|line| line.trim() == "repeated")
            .count()
            > 1,
        "{stdout}"
    );
    assert_eq!(
        stdout.lines().filter(|line| line.trim() == "once").count(),
        1,
        "{stdout}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_dispatches_foreground_task_events_in_order() {
    let active = root(None);
    let isolated = root(None);
    let (with_events, without_events) = tokio::join!(
        run(&active, "events", MONDAY, &["schedule:run"]),
        run(&isolated, "events-isolated", MONDAY, &["schedule:run"]),
    );
    let stdout = String::from_utf8_lossy(&with_events.stdout);
    let events: Vec<_> = stdout
        .lines()
        .filter(|line| {
            ["starting ", "finished ", "failed ", "skipped "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .map(|line| line.split_once(':').map_or(line, |(name, _)| name))
        .collect();
    assert_eq!(
        events,
        [
            "starting success",
            "finished success",
            "starting nonzero",
            "finished nonzero",
            "failed nonzero",
            "starting exception",
            "failed exception",
            "starting spawn-error",
            "failed spawn-error",
            "skipped filtered",
            "starting listener-error",
            "finished listener-error",
            "failed listener-error",
            "starting last",
            "finished last",
        ],
        "{stdout}"
    );
    assert!(stdout.contains("failed exception: scheduled callback error"));
    assert!(stdout.contains("failed listener-error: event listener failed"));
    assert!(
        stdout.find("failed exception:").unwrap()
            < stdout.find("scheduled failure reported").unwrap()
    );
    let other = String::from_utf8_lossy(&without_events.stdout);
    assert!(!other.contains("starting success"), "{other}");
    assert!(other.lines().any(|line| line.trim() == "last"), "{other}");
    std::fs::remove_dir_all(active).unwrap();
    std::fs::remove_dir_all(isolated).unwrap();
}

#[tokio::test]
async fn artisan_runs_task_hooks_in_order_with_group_inheritance() {
    let root = root(None);
    let result = run(&root, "hooks", MONDAY, &["schedule:run"]).await;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let trace: Vec<_> = stdout
        .lines()
        .filter(|line| {
            ["starting ", "hook ", "finished ", "failed "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .map(|line| line.split_once(':').map_or(line, |(name, _)| name))
        .collect();
    assert_eq!(
        trace,
        [
            "starting hook-success",
            "hook group-before",
            "hook nested-before",
            "hook task-before",
            "hook group-after",
            "hook group-success",
            "hook task-then",
            "finished hook-success",
            "starting hook-nonzero",
            "hook group-before",
            "hook group-after",
            "hook group-failure",
            "finished hook-nonzero",
            "failed hook-nonzero",
            "starting hook-callback-error",
            "hook group-before",
            "hook group-after",
            "hook group-failure",
            "failed hook-callback-error",
            "starting before-error",
            "failed before-error",
            "starting after-error",
            "failed after-error",
            "starting outside",
            "finished outside",
        ],
        "{stdout}"
    );
    assert!(stdout.contains("failed before-error: before hook error"));
    assert!(stdout.contains("failed after-error: after hook error"));
    assert!(!stdout.lines().any(|line| line == "before-error"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_redirects_scheduled_output_and_passes_failed_output_to_hooks() {
    let root = root(None);
    for name in ["overwrite.log", "append.log"] {
        std::fs::write(root.join(name), b"existing\n").unwrap();
    }
    for _ in 0..2 {
        let result = run(&root, "output", MONDAY, &["schedule:run"]).await;
        assert!(result.status.success(), "{result:?}");
        assert!(!String::from_utf8_lossy(&result.stdout).contains("artisan command"));
    }
    assert_eq!(
        std::fs::read(root.join("overwrite.log")).unwrap(),
        b"artisan command\n"
    );
    assert_eq!(
        std::fs::read(root.join("append.log")).unwrap(),
        b"existing\nartisan command\nartisan command\n"
    );
    let failure = std::fs::read(root.join("failure.log")).unwrap();
    assert!(!failure.is_empty());
    assert_eq!(
        std::fs::read(root.join("failure-hook.log")).unwrap(),
        failure
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_emails_scheduled_output_and_manually_runs_selected_commands() {
    let root = root(None);
    let scheduled = run(&root, "output-mail", MONDAY, &["schedule:run"]).await;
    let delivered = String::from_utf8_lossy(&scheduled.stderr);
    assert!(delivered.contains("success@example.test"), "{scheduled:?}");
    assert!(delivered.contains("failure@example.test"), "{scheduled:?}");
    assert!(
        delivered.contains("Scheduled success") && delivered.contains("artisan command"),
        "{scheduled:?}"
    );
    assert!(
        !delivered.contains("unexpected@example.test"),
        "{scheduled:?}"
    );
    let selected = run(
        &root,
        "basic",
        MONDAY + 3600,
        &["schedule:test", "--name=fixture:hello"],
    )
    .await;
    let selected = String::from_utf8_lossy(&selected.stdout);
    assert!(
        selected.contains("Running [fixture:hello]") && selected.contains("artisan command"),
        "{selected}"
    );
    let missing = run(&root, "basic", MONDAY, &["schedule:test", "--name=unknown"]).await;
    assert!(String::from_utf8_lossy(&missing.stdout).contains("No matching scheduled command"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn artisan_clears_redis_schedule_mutexes_for_registered_tasks() {
    let Ok(port) = std::env::var("OPENRUST_TEST_REDIS_PORT") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_REDIS").is_none());
        return;
    };
    let root = root(Some(port.parse().unwrap()));
    let winner = RunningScheduler::start(artisan(&root, "overlap", MONDAY, &["schedule:run"]));
    wait_for_receipts(&root, 1).await;
    let cleared = run(&root, "overlap", MONDAY, &["schedule:clear-cache"]).await;
    assert!(String::from_utf8_lossy(&cleared.stdout).contains("Deleting mutex for [shared]"));
    let empty = run(&root, "overlap", MONDAY, &["schedule:clear-cache"]).await;
    assert!(String::from_utf8_lossy(&empty.stdout).contains("No mutex files were found"));
    std::fs::write(root.join("release"), "").unwrap();
    let winner = timeout(Duration::from_secs(5), winner.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert_success(&winner);
    run(&root, "overlap", MONDAY, &["schedule:run"]).await;
    wait_for_receipts(&root, 2).await;
    std::fs::remove_dir_all(root).unwrap();
}
