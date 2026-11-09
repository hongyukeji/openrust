//! 从原核心 Provider 测试迁入，保留模块配置/依赖/Dispatcher 合同。
#![allow(
    clippy::unwrap_used,
    reason = "保留原受控测试日志锁断言，不用于生产代码"
)]

use figment::Figment;
use openrust::core::Result;
use openrust::events::{Dispatcher, EventServiceProvider, EventsServiceProvider};
use openrust::foundation::ProviderBuilder;
use openrust::foundation::ProviderId;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, DefaultProviders, ServiceProvider};
use openrust_modules::{
    ModuleDescriptor, ModuleManifest, ModuleProviderBuilderExt, ModuleRepository,
};
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn push(&self, message: impl Into<String>) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(message.into());
    }

    fn values(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

struct EventProbe;

impl openrust::events::Event for EventProbe {
    const NAME: &'static str = r"OpenRustModulesTests\EventProbe";
}

struct NestedEvents(Trace);

impl ServiceProvider for NestedEvents {
    fn register<'a>(&'a self, context: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let trace = self.0.clone();
            let mut events = EventsServiceProvider::new();
            events.listen::<EventProbe, _>(move |_, _| {
                let trace = trace.clone();
                Box::pin(async move {
                    trace.push("event.received");
                    Ok(())
                })
            });
            context.register_provider(events).await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn explicit_core_event_service_keeps_nested_listener_provider() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(EventServiceProvider);
    builder
        .module_manifest(ModuleManifest::new())
        .provider(NestedEvents(trace.clone()));
    let runtime = builder.start(Figment::new()).await?;
    let outcome = async {
        let typed = runtime.container().resolve::<Dispatcher>().await?;
        let named = runtime
            .container()
            .resolve_key::<Dispatcher>(openrust::container::ServiceKey::named("events"))
            .await?;
        assert!(Arc::ptr_eq(&typed, &named));
        runtime
            .container()
            .resolve::<Dispatcher>()
            .await?
            .dispatch(&EventProbe)
            .await
    }
    .await;
    let shutdown = runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await;
    outcome?;
    assert_eq!(trace.values(), ["event.received"]);
    shutdown
}

#[tokio::test]
async fn explicitly_removed_listener_provider_keeps_core_without_nested_listeners() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder.provider(EventServiceProvider);
    builder
        .module_manifest(ModuleManifest::new())
        .provider(NestedEvents(trace.clone()));
    builder.remove_provider(ProviderId::of::<EventsServiceProvider>());
    let runtime = builder.start(Figment::new()).await?;
    let bound = runtime.container().contains::<Dispatcher>();
    let shutdown = runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await;
    assert!(bound);
    assert_eq!(trace.values(), Vec::<String>::new());
    shutdown
}

#[tokio::test]
async fn modules_do_not_install_events_for_bare_or_explicitly_excluded_core() -> Result<()> {
    for excluded in [false, true] {
        let root = tempfile::tempdir().map_err(openrust::Error::internal)?;
        let statuses = root.path().join("statuses.json");
        let mut builder = ProviderBuilder::new();
        if excluded {
            builder.default_providers(
                DefaultProviders::empty()
                    .merge([Box::new(EventServiceProvider) as Box<dyn ServiceProvider>])
                    .except([ProviderId::of::<EventServiceProvider>()]),
            );
        }
        builder.module_manifest(
            ModuleManifest::from_file(&statuses).module(ModuleDescriptor::new(
                "Blog",
                root.path(),
                0,
            )),
        );
        let runtime = builder.start(Figment::new()).await?;
        let outcome: Result<()> = async {
            assert!(!runtime.container().contains::<Dispatcher>());
            assert!(
                !runtime
                    .container()
                    .contains_key(&openrust::container::ServiceKey::named("events"))
            );
            let repository = runtime.container().resolve::<ModuleRepository>().await?;
            assert_eq!(repository.all().len(), 1);
            let result = repository.enable("Blog").await;
            assert!(matches!(result, Err(error) if error.code() == "module_events_unavailable"));
            assert!(!statuses.exists());
            assert!(!repository.find_or_fail("Blog")?.is_enabled()?);
            Ok(())
        }
        .await;
        let cleanup = runtime
            .shutdown(Instant::now() + Duration::from_secs(2))
            .await;
        outcome?;
        cleanup?;
    }
    Ok(())
}
