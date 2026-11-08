use super::*;
use crate::events::{EventPropagation, EventsServiceProvider};
use crate::foundation::{ProviderBuilder, RegisterContext};
use figment::providers::Serialized;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
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
