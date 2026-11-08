use super::*;
use crate::events::{EventPropagation, EventsServiceProvider};
use crate::foundation::{ProviderBuilder, RegisterContext};
use figment::providers::Serialized;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::Notify;

type Trace = Arc<Mutex<Vec<&'static str>>>;

fn lock(trace: &Trace) -> MutexGuard<'_, Vec<&'static str>> {
    trace.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Probe(Trace);

struct ProbeCommand;

impl Command for ProbeCommand {
    fn name(&self) -> &'static str {
        "terminating-probe"
    }

    fn about(&self) -> &'static str {
        "验证命令终止回调"
    }

    fn run<'a>(&'a self, app: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.state()
                .resolve::<AtomicBool>()
                .await?
                .store(true, Ordering::Relaxed);
            Ok(())
        })
    }
}

impl ServiceProvider for Probe {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context
                .scoped::<AtomicBool, _>(|_| {
                    Box::pin(async { Ok(Arc::new(AtomicBool::new(false))) })
                })
                .await?;
            let trace = Arc::clone(&self.0);
            context.terminating(move |state| {
                let trace = Arc::clone(&trace);
                Box::pin(async move {
                    let ran = state.resolve::<AtomicBool>().await?.load(Ordering::Relaxed);
                    lock(&trace).push(if ran { "command-ran" } else { "no-command" });
                    Ok(())
                })
            });
            Ok(())
        })
    }

    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        context.command(ProbeCommand);
        context.terminating(record(&self.0, "boot"));
        Box::pin(async { Ok(()) })
    }
}

fn record(
    trace: &Trace,
    event: &'static str,
) -> impl Fn(ApplicationState) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static {
    let trace = Arc::clone(trace);
    move |_| {
        lock(&trace).push(event);
        Box::pin(async { Ok(()) })
    }
}

async fn build(builder: ProviderBuilder) -> Result<Application> {
    Application::from_provider_runtime(
        builder
            .start(Figment::from(Serialized::defaults(
                FrameworkConfig::default(),
            )))
            .await?,
    )
}

#[tokio::test]
async fn terminating_console_keeps_scope_order_and_primary_error() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder
        .terminating(record(&trace, "builder"))
        .provider(Probe(Arc::clone(&trace)));
    let app = build(builder).await?;
    app.run_command("terminating-probe", Vec::new()).await?;
    app.run_command("terminating-probe", vec!["--help".into()])
        .await?;
    let invalid = app
        .run_command("terminating-probe", vec!["bad".into()])
        .await;
    assert!(matches!(invalid, Err(error) if error.code() == "unexpected_argument"));
    assert_eq!(
        *lock(&trace),
        [
            "builder",
            "command-ran",
            "boot",
            "builder",
            "no-command",
            "boot",
            "builder",
            "no-command",
            "boot",
        ]
    );

    app.terminating(|_| {
        Box::pin(async { Err(Error::custom("test_termination_failed", "终止失败")) })
    });
    app.terminating(record(&trace, "unreachable"));
    let unknown = app.run_command("does-not-exist", Vec::new()).await;
    assert!(matches!(unknown, Err(error) if error.code() == "unknown_command"));
    let successful = app.run_command("terminating-probe", Vec::new()).await;
    assert!(matches!(successful, Err(error) if error.code() == "test_termination_failed"));
    assert_eq!(
        &lock(&trace)[9..],
        [
            "builder",
            "no-command",
            "boot",
            "builder",
            "command-ran",
            "boot"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn console_duration_handlers_observe_completion_snapshot_and_primary_error() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    let trace = Trace::default();
    builder.provider(Probe(Arc::clone(&trace)));
    let mut app = build(builder).await?;
    assert!(app.state().command_started_at().is_none());
    app.config_repository()
        .set("app.timezone", serde_json::json!("Asia/Shanghai"))?;

    let probe_trace = Arc::clone(&trace);
    let first_elapsed = Arc::new(Mutex::new(None));
    let elapsed = Arc::clone(&first_elapsed);
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::ZERO, move |state, command| {
            let trace = Arc::clone(&probe_trace);
            let elapsed = Arc::clone(&elapsed);
            Box::pin(async move {
                assert_eq!(command.name, "terminating-probe");
                assert_eq!(command.status, i32::from(command.arguments == ["bad"]));
                assert_eq!(state.request_started_at(), None);
                assert_eq!(
                    state
                        .command_started_at()
                        .map(|time| time.timestamp_micros()),
                    Some(command.started_at.try_chrono()?.timestamp_micros()),
                );
                assert_eq!(command.started_at.format("e")?, "Asia/Shanghai");
                assert_eq!(lock(&trace).last(), Some(&"boot"));
                assert_eq!(
                    state.resolve::<AtomicBool>().await?.load(Ordering::Relaxed),
                    command.arguments.is_empty()
                );
                *elapsed.lock().unwrap_or_else(PoisonError::into_inner) = Some(command.elapsed);
                tokio::task::yield_now().await;
                lock(&trace).push("first-duration");
                Ok(())
            })
        });
    let probe_trace = Arc::clone(&trace);
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::MAX, |_, _| {
            Box::pin(async { Err(Error::custom("test_threshold", "不应触发该阈值")) })
        });
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::ZERO, move |_, command| {
            let trace = Arc::clone(&probe_trace);
            let elapsed = Arc::clone(&first_elapsed);
            Box::pin(async move {
                assert_eq!(
                    Some(command.elapsed),
                    *elapsed.lock().unwrap_or_else(PoisonError::into_inner)
                );
                lock(&trace).push("second-duration");
                if command.status != 0 {
                    return Err(Error::custom("test_duration_error", "时长处理器失败"));
                }
                Ok(())
            })
        });
    let probe_trace = Arc::clone(&trace);
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::ZERO, move |_, _| {
            lock(&probe_trace).push("third-duration");
            Box::pin(async { Ok(()) })
        });
    app.run_command("terminating-probe", Vec::new()).await?;
    app.run_command("terminating-probe", vec!["--help".into()])
        .await?;
    let invalid = app
        .run_command("terminating-probe", vec!["bad".into()])
        .await;
    assert!(matches!(invalid, Err(error) if error.code() == "unexpected_argument"));
    assert!(app.state().command_started_at().is_none());
    assert_eq!(
        *lock(&trace),
        [
            "command-ran",
            "boot",
            "first-duration",
            "second-duration",
            "third-duration",
            "no-command",
            "boot",
            "first-duration",
            "second-duration",
            "third-duration",
            "no-command",
            "boot",
            "first-duration",
            "second-duration",
        ]
    );
    app.terminate().await?;
    assert_eq!(&lock(&trace)[14..], ["no-command", "boot"]);
    app.flush()?;
    assert!(
        app.run_command("terminating-probe", Vec::new())
            .await
            .is_err()
    );
    assert_eq!(lock(&trace).len(), 16);
    Ok(())
}

struct ConcurrentCommand(Arc<tokio::sync::Barrier>);

impl Command for ConcurrentCommand {
    fn name(&self) -> &'static str {
        "concurrent-timing"
    }

    fn about(&self) -> &'static str {
        "验证并行命令的入口计时隔离"
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, _: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let before = app.state().command_started_at();
            assert!(before.is_some());
            self.0.wait().await;
            assert_eq!(app.state().command_started_at(), before);
            Ok(())
        })
    }
}

impl ServiceProvider for ConcurrentCommand {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        app.command(Self(Arc::clone(&self.0)));
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn simultaneous_commands_keep_their_own_started_at() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(ConcurrentCommand(Arc::new(tokio::sync::Barrier::new(2))));
    let app = build(builder).await?;
    let first = EntryStarted::now();
    let mut second = first;
    second.at += chrono::TimeDelta::seconds(1);
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::ZERO, move |state, command| {
            Box::pin(async move {
                let expected = if command.arguments == ["first"] {
                    first.at
                } else {
                    second.at
                };
                assert_eq!(state.command_started_at(), Some(expected));
                assert_eq!(
                    command.started_at.try_chrono()?.timestamp_micros(),
                    expected.timestamp_micros()
                );
                Ok(())
            })
        });
    let (one, two) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            app.run_command_since("concurrent-timing", vec!["first".into()], first),
            app.run_command_since("concurrent-timing", vec!["second".into()], second),
        )
    })
    .await
    .map_err(Error::internal)?;
    one?;
    two?;
    assert!(app.state().command_started_at().is_none());
    Ok(())
}

#[tokio::test]
async fn duration_snapshot_includes_app_registrations_and_defers_handler_registrations()
-> Result<()> {
    let mut builder = ProviderBuilder::new();
    let trace = Trace::default();
    builder.provider(Probe(Arc::clone(&trace)));
    let app = build(builder).await?;
    let appended = AtomicBool::new(false);
    let captured = Arc::clone(&trace);
    app.terminating(move |state| {
        let captured = Arc::clone(&captured);
        if !appended.swap(true, Ordering::Relaxed) {
            let late = Arc::clone(&captured);
            let registered = AtomicBool::new(false);
            state.when_command_lifecycle_is_longer_than(Duration::ZERO, move |state, _| {
                lock(&captured).push("during-termination");
                let late = Arc::clone(&late);
                if !registered.swap(true, Ordering::Relaxed) {
                    state.when_command_lifecycle_is_longer_than(Duration::ZERO, move |_, _| {
                        lock(&late).push("during-duration");
                        Box::pin(async { Ok(()) })
                    });
                }
                Box::pin(async { Ok(()) })
            });
        }
        Box::pin(async { Ok(()) })
    });
    app.run_command("terminating-probe", Vec::new()).await?;
    assert_eq!(*lock(&trace), ["command-ran", "boot", "during-termination"]);
    app.run_command("terminating-probe", Vec::new()).await?;
    assert_eq!(
        &lock(&trace)[3..],
        [
            "command-ran",
            "boot",
            "during-termination",
            "during-duration"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn duration_termination_validates_timezone_before_threshold_filtering() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    let trace = Trace::default();
    builder.provider(Probe(Arc::clone(&trace)));
    let app = build(builder).await?;
    app.state()
        .when_command_lifecycle_is_longer_than(Duration::MAX, |_, _| {
            Box::pin(async {
                Err(Error::custom("test_unreachable", "超出测试可等待时长"))
            })
        });
    app.config_repository()
        .set("app.timezone", serde_json::json!("Invalid/Timezone"))?;
    let result = app.run_command("terminating-probe", Vec::new()).await;
    assert!(matches!(result, Err(error) if error.code() == "invalid_config"));
    assert_eq!(*lock(&trace), ["command-ran", "boot"]);
    Ok(())
}

#[tokio::test]
async fn terminating_replays_dynamic_queue_and_flush_clears_it() -> Result<()> {
    let mut app = build(ProviderBuilder::new()).await?;
    let trace = Trace::default();
    let captured = Arc::clone(&trace);
    app.terminating(move |state| {
        let mut entries = lock(&captured);
        if entries.is_empty() {
            state.terminating(record(&captured, "appended"));
        }
        entries.push("first");
        Box::pin(async { Ok(()) })
    });
    app.terminating(record(&trace, "second"));
    app.terminate().await?;
    app.terminate().await?;
    assert_eq!(
        *lock(&trace),
        ["first", "second", "appended", "first", "second", "appended"]
    );
    app.flush()?;
    app.terminate().await?;
    app.terminating(record(&trace, "after-flush"));
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    assert_eq!(lock(&trace).len(), 6);
    Ok(())
}

#[tokio::test]
async fn terminating_flush_during_callback_keeps_new_generation_for_next_pass() -> Result<()> {
    let mut app = build(ProviderBuilder::new()).await?;
    let state = app.state().clone();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let ready = Arc::clone(&entered);
    let resume = Arc::clone(&release);
    app.terminating(move |_| {
        let ready = Arc::clone(&ready);
        let resume = Arc::clone(&resume);
        Box::pin(async move {
            ready.notify_one();
            resume.notified().await;
            Ok(())
        })
    });
    let trace = Trace::default();
    app.terminating(record(&trace, "old-second"));
    let running = tokio::spawn(async move { state.run_terminating().await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    app.flush()?;
    app.terminating(record(&trace, "new-first"));
    app.terminating(record(&trace, "new-second"));
    release.notify_one();
    running.await.map_err(Error::internal)??;
    assert!(lock(&trace).is_empty());
    app.terminate().await?;
    assert_eq!(*lock(&trace), ["new-first", "new-second"]);
    Ok(())
}

#[tokio::test]
async fn kernel_terminating_dispatches_shared_event_before_console_callbacks() -> Result<()> {
    let trace = Trace::default();
    let mut events = EventsServiceProvider::new();
    let typed = Arc::clone(&trace);
    events.listen::<Terminating, _>(move |_, context| {
        assert_eq!(
            context.event_name(),
            r"Illuminate\Foundation\Events\Terminating"
        );
        lock(&typed).push("typed");
        Box::pin(async { Ok(()) })
    });
    let wildcard = Arc::clone(&trace);
    events.listen_named::<Terminating, _>(r"Illuminate\Foundation\Events\*", move |_, _| {
        lock(&wildcard).push("wildcard");
        Box::pin(async { Ok(EventPropagation::Stop) })
    });
    events.listen_named::<Terminating, _>("*", |_, _| {
        Box::pin(async { Err(Error::custom("test_unexpected_listener", "传播本应已停止")) })
    });
    let mut builder = ProviderBuilder::new();
    builder.provider(events).provider(Probe(Arc::clone(&trace)));
    let app = build(builder).await?;
    app.run_command("terminating-probe", Vec::new()).await?;
    app.run_command("terminating-probe", vec!["--help".into()])
        .await?;
    let unknown = app.run_command("missing", Vec::new()).await;
    assert!(matches!(unknown, Err(error) if error.code() == "unknown_command"));
    assert_eq!(
        *lock(&trace),
        [
            "typed",
            "wildcard",
            "command-ran",
            "boot",
            "typed",
            "wildcard",
            "no-command",
            "boot",
            "typed",
            "wildcard",
            "no-command",
            "boot",
        ]
    );
    Ok(())
}

#[tokio::test]
async fn kernel_terminating_listener_error_skips_callbacks_but_manual_terminate_still_runs()
-> Result<()> {
    let trace = Trace::default();
    let mut events = EventsServiceProvider::new();
    events.listen::<Terminating, _>(|_, _| {
        Box::pin(async { Err(Error::custom("test_listener_failed", "终止事件监听失败")) })
    });
    let mut builder = ProviderBuilder::new();
    builder.provider(events).provider(Probe(Arc::clone(&trace)));
    let app = build(builder).await?;
    let success = app.run_command("terminating-probe", Vec::new()).await;
    assert!(matches!(success, Err(error) if error.code() == "test_listener_failed"));
    let unknown = app.run_command("missing", Vec::new()).await;
    assert!(matches!(unknown, Err(error) if error.code() == "unknown_command"));
    assert!(lock(&trace).is_empty());
    app.terminate().await?;
    assert_eq!(*lock(&trace), ["no-command", "boot"]);
    Ok(())
}
