use super::*;
use crate::foundation::Application;
use figment::providers::Serialized;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

type Trace = Arc<Mutex<Vec<&'static str>>>;

struct Retired(Arc<AtomicUsize>);

impl Drop for Retired {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn record(trace: &Trace, item: &'static str) {
    trace
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(item);
}

fn hook(
    trace: &Trace,
    item: &'static str,
) -> impl for<'b> Fn(&'b mut BootContext<'_>) -> BoxFuture<'b, Result<()>> + Send + Sync + 'static {
    let trace = Arc::clone(trace);
    move |_| {
        record(&trace, item);
        Box::pin(async { Ok(()) })
    }
}

struct StartupProbe(Trace);

impl ServiceProvider for StartupProbe {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            record(&self.0, "register");
            context.booting(hook(&self.0, "register-booting"));
            context.booted(hook(&self.0, "register-booted")).await
        })
    }

    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            record(&self.0, "provider-boot");
            context.booted(hook(&self.0, "provider-booted")).await
        })
    }
}

#[tokio::test]
async fn application_startup_callbacks_follow_the_dynamic_queue_and_late_registration() -> Result<()>
{
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(StartupProbe(Arc::clone(&trace)));
    let observed = Arc::clone(&trace);
    builder.booting(move |context| {
        record(&observed, "builder-booting");
        context.booting(hook(&observed, "nested-booting"));
        Box::pin(async { Ok(()) })
    });
    let observed = Arc::clone(&trace);
    builder.booted(move |context| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            record(&observed, "builder-booted-start");
            context.booted(hook(&observed, "nested-booted")).await?;
            record(&observed, "builder-booted-end");
            Ok(())
        })
    });
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
    let mut app = Application::from_provider_runtime(builder.start(source).await?)?;
    app.booting(hook(&trace, "late-booting"));
    app.booted(hook(&trace, "late-booted")).await?;
    assert_eq!(
        *trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            "register",
            "builder-booting",
            "register-booting",
            "nested-booting",
            "provider-boot",
            "builder-booted-start",
            "nested-booted",
            "builder-booted-end",
            "register-booted",
            "provider-booted",
            "nested-booted",
            "late-booted",
        ]
    );

    let retired = Arc::new(AtomicUsize::new(0));
    let held = Retired(Arc::clone(&retired));
    app.booted(move |_| {
        let _captured = &held;
        Box::pin(async { Ok(()) })
    })
    .await?;
    assert_eq!(retired.load(Ordering::Relaxed), 0);
    app.flush()?;
    assert_eq!(retired.load(Ordering::Relaxed), 1);
    app.register_provider(StartupProbe(Arc::clone(&trace)))
        .await?;
    {
        let actual = trace
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            &actual[12..],
            [
                "register",
                "register-booted",
                "provider-boot",
                "provider-booted"
            ]
        );
    }
    app.shutdown(Instant::now() + Duration::from_secs(2)).await
}

#[tokio::test]
async fn application_booted_failure_uses_the_existing_startup_cleanup() -> Result<()> {
    let cleaned = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&cleaned);
    let mut builder = ProviderBuilder::new();
    builder.booting(move |context| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            context.resources().on_shutdown(move || {
                Box::pin(async move {
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            })
        })
    });
    builder.booted(|_| {
        Box::pin(async { Err(Error::custom("test_booted_failure", "应用 booted 回调失败")) })
    });
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
    assert!(
        matches!(builder.start(source).await, Err(error) if error.code() == "test_booted_failure")
    );
    assert_eq!(cleaned.load(Ordering::Relaxed), 1);
    Ok(())
}
