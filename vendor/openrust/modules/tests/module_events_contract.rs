//! 从原核心 Provider 测试迁入，保留模块配置/依赖/Events 合同。
#![allow(
    clippy::unwrap_used,
    reason = "保留原受控测试日志锁断言，不用于生产代码"
)]

use figment::Figment;
use openrust::core::Result;
use openrust::events::{Events, EventsServiceProvider};
use openrust::foundation::ProviderBuilder;
use openrust::foundation::ProviderId;
use openrust::foundation::RegisterContext;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust_modules::{ModuleManifest, ModuleProviderBuilderExt};
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
async fn default_module_event_service_does_not_swallow_nested_listener_provider() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder
        .module_manifest(ModuleManifest::new())
        .provider(NestedEvents(trace.clone()));
    let runtime = builder.start(Figment::new()).await?;
    let outcome = async {
        runtime
            .container()
            .resolve::<Events>()
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
async fn explicitly_removed_real_events_suppresses_default_and_nested_registration() -> Result<()> {
    let trace = Trace::default();
    let mut builder = ProviderBuilder::new();
    builder
        .module_manifest(ModuleManifest::new())
        .provider(NestedEvents(trace.clone()));
    builder.remove_provider(ProviderId::of::<EventsServiceProvider>());
    let runtime = builder.start(Figment::new()).await?;
    let bound = runtime.container().contains::<Events>();
    let shutdown = runtime
        .shutdown(Instant::now() + Duration::from_secs(2))
        .await;
    assert!(!bound);
    assert_eq!(trace.values(), Vec::<String>::new());
    shutdown
}
