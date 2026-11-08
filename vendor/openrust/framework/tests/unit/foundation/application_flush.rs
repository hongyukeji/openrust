use super::*;
use crate::foundation::{BootContext, ProviderBuilder, RegisterContext};
use crate::support::BoxFuture;
use figment::providers::Serialized;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct BootValue(usize);

struct Provider {
    value: usize,
    registrations: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
}

impl ServiceProvider for Provider {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.registrations.fetch_add(1, Ordering::Relaxed);
            context.instance(Arc::new(self.value)).await
        })
    }

    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context.extension_mut::<BootValue>()?.0 = self.value;
            context.command(crate::console::command::ListCommand);
            let closed = Arc::clone(&self.closed);
            context.resources().on_shutdown(move || {
                Box::pin(async move {
                    closed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            })
        })
    }
}

async fn start(provider: impl ServiceProvider) -> Result<Application> {
    let mut builder = ProviderBuilder::new();
    builder.provider(provider);
    let sources = Figment::from(Serialized::defaults(FrameworkConfig::default()));
    Application::from_provider_runtime(builder.start(sources).await?)
}

#[tokio::test]
async fn application_flush_clears_registrations_and_keeps_resource_ownership() -> Result<()> {
    let registrations = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));
    let provider = |value| Provider {
        value,
        registrations: Arc::clone(&registrations),
        closed: Arc::clone(&closed),
    };
    let mut app = start(provider(7)).await?;
    let old_state = app.state().clone();
    let container = old_state.container();
    let old_service = old_state.resolve::<usize>().await?;
    let old_command = app
        .commands()
        .next()
        .ok_or_else(|| Error::custom("test_command_missing", "缺少启动命令"))?;
    let resolutions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&resolutions);
    container.before_resolving(Some(ServiceKey::typed::<usize>()), move |_, _| {
        observed.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    })?;
    let rebounds = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&rebounds);
    container
        .rebinding::<usize, _>(move |_, _| {
            observed.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        })
        .await?;
    container
        .extend::<i64, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 2)) }))
        .await?;
    app.config_repository()
        .set("probe.kept", serde_json::json!(true))?;
    let before = resolutions.load(Ordering::Relaxed);
    assert!(before > 0);

    app.flush()?;
    assert!(!container.contains::<usize>());
    assert!(app.extension::<BootValue>().is_none());
    assert!(app.commands().next().is_none());
    assert_eq!(old_command.name(), "list");
    assert_eq!(closed.load(Ordering::Relaxed), 0);
    assert!(app.section::<bool>("probe.kept")?);

    app.register_provider(provider(9)).await?;
    assert_eq!(app.extension::<BootValue>().map(|value| value.0), Some(9));
    assert!(app.commands().any(|command| command.name() == "list"));
    assert_eq!(*old_state.resolve::<usize>().await?, 9);
    assert_eq!(*old_service, 7);
    container.instance(Arc::new(11_usize)).await?;
    assert_eq!(resolutions.load(Ordering::Relaxed), before);
    assert_eq!(rebounds.load(Ordering::Relaxed), 0);
    container
        .bind::<i64, _>(|_| Box::pin(async { Ok(Arc::new(40)) }))
        .await?;
    assert_eq!(*container.resolve::<i64>().await?, 42);
    assert_eq!(registrations.load(Ordering::Relaxed), 2);

    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    assert_eq!(closed.load(Ordering::Relaxed), 2);
    assert!(matches!(app.flush(), Err(error) if error.code() == "container_closed"));
    assert!(app.commands().next().is_some());
    Ok(())
}

#[tokio::test]
async fn application_flush_retires_all_hooks_and_host_objects_when_destructors_fail() -> Result<()>
{
    struct FailOnDrop(Arc<AtomicUsize>);
    impl Drop for FailOnDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            std::panic::resume_unwind(Box::new("测试应用清空时的析构异常"));
        }
    }
    let registrations = Arc::new(AtomicUsize::new(0));
    let mut app = start(Provider {
        value: 1,
        registrations,
        closed: Arc::new(AtomicUsize::new(0)),
    })
    .await?;
    let retired = Arc::new(AtomicUsize::new(0));
    let container = app.state().container().clone();
    for _ in 0..2 {
        let captured = FailOnDrop(Arc::clone(&retired));
        container.after_resolving(None, move |_, _| {
            let _retained = &captured;
            Box::pin(async { Ok(()) })
        })?;
    }
    assert!(matches!(app.flush(), Err(error) if error.code() == "resource_task_interrupted"));
    assert_eq!(retired.load(Ordering::Relaxed), 2);
    assert!(app.commands().next().is_none());
    assert!(app.extension::<BootValue>().is_none());
    assert!(!container.contains::<usize>());
    app.flush()?;
    app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    Ok(())
}
