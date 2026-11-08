use super::*;
use crate::foundation::Application;
use figment::providers::Serialized;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

type Trace = Arc<Mutex<Vec<(&'static str, &'static str)>>>;

fn trace(log: &Trace, instance: &'static str, event: &'static str) {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((instance, event));
}

fn hook(
    log: &Trace,
    instance: &'static str,
    event: &'static str,
) -> impl for<'a> Fn(&'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> + Send + Sync + 'static {
    let log = Arc::clone(log);
    move |_| {
        trace(&log, instance, event);
        Box::pin(async { Ok(()) })
    }
}

struct Retired(Arc<AtomicUsize>);
struct BootDependency;

impl Drop for Retired {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
enum Failure {
    None,
    Register,
    Booted,
}

struct Probe<const ID: u8> {
    name: &'static str,
    log: Trace,
    retired: Arc<AtomicUsize>,
    failure: Failure,
}

impl<const ID: u8> ServiceProvider for Probe<ID> {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            trace(&self.log, self.name, "register");
            let name = self.name;
            let log = Arc::clone(&self.log);
            let retired = Retired(Arc::clone(&self.retired));
            context.provider_booting(move |app| {
                let _retained = &retired;
                let log = Arc::clone(&log);
                Box::pin(async move {
                    app.resolve::<BootDependency>().await?;
                    trace(&log, name, "booting");
                    app.provider_booting(hook(&log, name, "nested-booting"))
                })
            });
            let log = Arc::clone(&self.log);
            let failure = self.failure;
            context.provider_booted(move |app| {
                trace(&log, name, "booted");
                let result = app.provider_booted(hook(&log, name, "nested-booted"));
                trace(&log, name, "booted-end");
                Box::pin(async move {
                    result?;
                    if matches!(failure, Failure::Booted) {
                        return Err(Error::custom(
                            "test_provider_booted_failure",
                            "Provider booted 失败",
                        ));
                    }
                    Ok(())
                })
            });
            if matches!(self.failure, Failure::Register) {
                return Err(Error::custom(
                    "test_provider_register_failure",
                    "Provider register 失败",
                ));
            }
            context.instance(Arc::new(BootDependency)).await
        })
    }

    fn boot<'a>(&'a self, context: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            trace(&self.log, self.name, "boot");
            context.provider_booting(hook(&self.log, self.name, "missed-booting"))?;
            context.provider_booted(hook(&self.log, self.name, "added-in-boot"))
        })
    }
}

fn probe<const ID: u8>(
    name: &'static str,
    log: &Trace,
    retired: &Arc<AtomicUsize>,
    failure: Failure,
) -> Probe<ID> {
    Probe {
        name,
        log: Arc::clone(log),
        retired: Arc::clone(retired),
        failure,
    }
}

#[tokio::test]
async fn instance_callbacks_follow_boot_and_do_not_transfer_on_force() -> Result<()> {
    let log = Trace::default();
    let retired = Arc::new(AtomicUsize::new(0));
    let mut builder = ProviderBuilder::new();
    builder.booting(hook(&log, "app", "booting"));
    builder.booted(hook(&log, "app", "booted"));
    builder.provider(probe::<1>("old", &log, &retired, Failure::None));
    builder.provider(probe::<2>("second", &log, &retired, Failure::None));
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
    let mut app = Application::from_provider_runtime(builder.start(source).await?)?;
    {
        let seen = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            &seen[..3],
            [
                ("old", "register"),
                ("second", "register"),
                ("app", "booting")
            ]
        );
        assert_eq!(seen.last(), Some(&("app", "booted")));
    }

    let candidate_retired = Arc::new(AtomicUsize::new(0));
    let failed = app
        .force_register_provider(probe::<1>(
            "failed",
            &log,
            &candidate_retired,
            Failure::Register,
        ))
        .await;
    assert!(matches!(failed, Err(error) if error.code() == "test_provider_register_failure"));
    assert_eq!(candidate_retired.load(Ordering::Relaxed), 1);
    assert_eq!(retired.load(Ordering::Relaxed), 0);
    app.register_provider(probe::<1>("duplicate", &log, &retired, Failure::None))
        .await?;
    app.force_register_provider(probe::<1>("replacement", &log, &retired, Failure::None))
        .await?;
    assert_eq!(retired.load(Ordering::Relaxed), 1);
    app.flush()?;
    assert_eq!(retired.load(Ordering::Relaxed), 3);

    let phases = [
        "register",
        "booting",
        "nested-booting",
        "boot",
        "booted",
        "booted-end",
        "added-in-boot",
        "nested-booted",
    ];
    {
        let seen = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for name in ["old", "second", "replacement"] {
            let events: Vec<_> = seen
                .iter()
                .filter_map(|&(owner, event)| (owner == name).then_some(event))
                .collect();
            assert_eq!(events, phases);
        }
        assert!(!seen.iter().any(|&(name, _)| name == "duplicate"));
        assert_eq!(
            seen.iter().filter(|&&(name, _)| name == "failed").count(),
            1
        );
    }
    app.shutdown(Instant::now() + Duration::from_secs(2)).await
}

#[tokio::test]
async fn live_provider_booted_failure_keeps_the_registered_instance() -> Result<()> {
    let source = Figment::from(Serialized::defaults(FrameworkConfig::default()));
    let mut app = Application::from_provider_runtime(ProviderBuilder::new().start(source).await?)?;
    let log = Trace::default();
    let retired = Arc::new(AtomicUsize::new(0));
    let failed = app
        .register_provider(probe::<1>("failed-booted", &log, &retired, Failure::Booted))
        .await;
    assert!(matches!(failed, Err(error) if error.code() == "test_provider_booted_failure"));
    app.register_provider(probe::<1>("duplicate", &log, &retired, Failure::None))
        .await?;
    {
        let seen = log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(!seen.iter().any(|&(name, _)| name == "duplicate"));
        assert_eq!(seen.last(), Some(&("failed-booted", "booted-end")));
    }
    app.flush()?;
    assert_eq!(retired.load(Ordering::Relaxed), 1);
    app.shutdown(Instant::now() + Duration::from_secs(2)).await
}
