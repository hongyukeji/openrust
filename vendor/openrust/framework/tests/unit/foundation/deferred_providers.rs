//! 延迟 Provider 的入口触发、启动次序和完成屏障回归。

use super::deferred::DeferrableProvider;
use crate::container::ServiceKey;
use crate::foundation::{Application, BootContext, ProviderBuilder, RegisterContext};
use crate::support::ServiceProvider;
use crate::{BoxFuture, Error, Result};
use figment::Figment;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;
use tokio::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<&'static str>>>);

impl Trace {
    fn push(&self, value: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(value);
    }

    fn values(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[derive(Debug)]
struct DeferredValue(u8);

fn named() -> ServiceKey {
    ServiceKey::named("tests.deferred.other")
}

struct DeferredProbe {
    trace: Trace,
    pause: Option<Arc<(Notify, Notify)>>,
}

impl DeferrableProvider for DeferredProbe {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<DeferredValue>(), named()]
    }

    fn when() -> Vec<&'static str> {
        vec![
            <ProbeEvent as crate::events::Event>::NAME,
            "tests.deferred.*",
        ]
    }
}

impl ServiceProvider for DeferredProbe {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("lazy.register");
            app.config_section(
                "deferred_fixture",
                serde_json::json!({"enabled": true}),
                crate::config::repository::SectionSchema::any(),
                |_| Ok(()),
            )?;
            app.merge_defaults("deferred_fixture", serde_json::json!({"ready": true}))?;
            app.instance(Arc::new(DeferredValue(7))).await?;
            app.instance_key(named(), Arc::new(DeferredValue(9))).await
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let settings = app.section::<serde_json::Value>("deferred_fixture")?;
            assert_eq!(settings["ready"], true);
            self.trace.push("lazy.boot");
            if let Some(pause) = &self.pause {
                pause.0.notify_one();
                pause.1.notified().await;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn shared_application_state_can_load_after_the_assembly_handle_is_released() -> Result<()> {
    let trace = Trace::default();
    let state = start(builder(&trace, None)).await?.state().clone();
    assert_eq!(state.resolve::<DeferredValue>().await?.0, 7);
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    state
        .shutdown_resources(Instant::now() + Duration::from_secs(2))
        .await
}

fn builder(trace: &Trace, pause: Option<Arc<(Notify, Notify)>>) -> ProviderBuilder {
    let mut builder = ProviderBuilder::new();
    let trace = trace.clone();
    builder.deferred_provider(move || DeferredProbe {
        trace: trace.clone(),
        pause: pause.clone(),
    });
    builder
}

async fn start(builder: ProviderBuilder) -> Result<Application> {
    Application::from_provider_runtime(builder.start(Figment::new()).await?)
}

async fn close(app: &Application) -> Result<()> {
    app.shutdown(Instant::now() + Duration::from_secs(2)).await
}

#[tokio::test]
async fn typed_named_alias_and_existing_instance_follow_deferred_directory() -> Result<()> {
    let trace = Trace::default();
    let app = start(builder(&trace, None)).await?;
    let container = app.state().container();
    let typed = ServiceKey::typed::<DeferredValue>();
    let alias = ServiceKey::named("tests.deferred.alias");
    assert!(app.is_deferred_service(&typed));
    assert!(container.contains_key(&typed));
    assert_eq!(trace.values(), [] as [&str; 0]);
    container.alias(typed.clone(), alias.clone())?;
    let existing = Arc::new(DeferredValue(3));
    container.instance(Arc::clone(&existing)).await?;
    assert!(Arc::ptr_eq(
        &container.resolve::<DeferredValue>().await?,
        &existing
    ));
    assert_eq!(trace.values(), [] as [&str; 0]);
    assert_eq!(container.resolve_key::<DeferredValue>(named()).await?.0, 9);
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    assert_eq!(container.resolve_key::<DeferredValue>(alias).await?.0, 7);
    assert!(app.is_deferred_service(&typed));
    assert!(!app.is_deferred_service(&named()));
    app.load_deferred_providers().await?;
    assert!(!app.is_deferred_service(&typed));
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    close(&app).await
}

struct EagerTrigger(Trace);

impl ServiceProvider for EagerTrigger {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("eager.begin");
            app.resolve::<DeferredValue>().await?;
            self.0.push("eager.end");
            Ok(())
        })
    }
}

#[tokio::test]
async fn startup_booting_trigger_boots_twice_but_eager_boot_trigger_boots_once() -> Result<()> {
    let trace = Trace::default();
    let mut booting = builder(&trace, None);
    let observed = trace.clone();
    booting.booting(move |app| {
        let observed = observed.clone();
        Box::pin(async move {
            observed.push("booting.begin");
            app.resolve::<DeferredValue>().await?;
            observed.push("booting.end");
            Ok(())
        })
    });
    let app = start(booting).await?;
    assert_eq!(
        trace.values(),
        [
            "booting.begin",
            "lazy.register",
            "booting.end",
            "lazy.boot",
            "lazy.boot"
        ]
    );
    close(&app).await?;

    let trace = Trace::default();
    let mut eager = builder(&trace, None);
    eager.provider(EagerTrigger(trace.clone()));
    let app = start(eager).await?;
    assert_eq!(
        trace.values(),
        ["eager.begin", "lazy.register", "eager.end", "lazy.boot"]
    );
    close(&app).await
}

#[tokio::test]
async fn published_instance_does_not_bypass_in_flight_provider_boot() -> Result<()> {
    let trace = Trace::default();
    let pause = Arc::new((Notify::new(), Notify::new()));
    let app = start(builder(&trace, Some(Arc::clone(&pause)))).await?;
    let container = app.state().container();
    let first_container = container.clone();
    let first = tokio::spawn(async move { first_container.resolve::<DeferredValue>().await });
    tokio::time::timeout(Duration::from_secs(2), pause.0.notified())
        .await
        .map_err(Error::internal)?;
    let second_container = container.clone();
    let mut second = tokio::spawn(async move { second_container.resolve::<DeferredValue>().await });
    let early = tokio::time::timeout(Duration::from_millis(40), &mut second).await;
    pause.1.notify_one();
    assert!(
        early.is_err(),
        "已发布 instance 不得越过同 Provider 的 boot 屏障"
    );
    let one = first.await.map_err(Error::internal)??;
    let two = second.await.map_err(Error::internal)??;
    assert!(Arc::ptr_eq(&one, &two));
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    close(&app).await
}

struct FailingProbe(Trace);

impl DeferrableProvider for FailingProbe {
    fn provides() -> Vec<ServiceKey> {
        vec![
            ServiceKey::named("tests.fail.first"),
            ServiceKey::named("tests.fail.second"),
        ]
    }
}

impl ServiceProvider for FailingProbe {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("failed.register");
            app.instance(Arc::new(DeferredValue(5))).await?;
            Err(Error::custom(
                "deferred_register_failed",
                "延迟注册失败探针",
            ))
        })
    }
}

#[tokio::test]
async fn registration_failure_consumes_only_trigger_key_and_keeps_partial_contribution()
-> Result<()> {
    let trace = Trace::default();
    let captured = trace.clone();
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || FailingProbe(captured.clone()));
    let app = start(builder).await?;
    for (name, attempts) in [
        ("tests.fail.first", 1),
        ("tests.fail.first", 1),
        ("tests.fail.second", 2),
        ("tests.fail.second", 2),
    ] {
        let key = ServiceKey::named(name);
        assert!(
            app.state()
                .container()
                .resolve_key::<DeferredValue>(key.clone())
                .await
                .is_err()
        );
        assert!(!app.is_deferred_service(&key));
        assert_eq!(trace.values().len(), attempts);
        assert_eq!(app.state().resolve::<DeferredValue>().await?.0, 5);
    }
    close(&app).await
}

#[tokio::test]
async fn flush_removes_untriggered_deferred_directory_without_running_factory() -> Result<()> {
    let trace = Trace::default();
    let mut app = start(builder(&trace, None)).await?;
    app.flush()?;
    assert!(!app.is_deferred_service(&ServiceKey::typed::<DeferredValue>()));
    assert!(!app.is_deferred_service(&named()));
    app.load_deferred_providers().await?;
    assert_eq!(trace.values(), [] as [&str; 0]);
    assert!(app.state().resolve::<DeferredValue>().await.is_err());
    close(&app).await
}

#[tokio::test]
async fn alias_first_resolution_triggers_typed_provider_and_explicit_load_all_is_once() -> Result<()>
{
    let trace = Trace::default();
    let app = start(builder(&trace, None)).await?;
    let alias = ServiceKey::named("tests.deferred.first-alias");
    app.state()
        .container()
        .alias(ServiceKey::typed::<DeferredValue>(), alias.clone())?;
    assert_eq!(
        app.state()
            .container()
            .resolve_key::<DeferredValue>(alias)
            .await?
            .0,
        7
    );
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    close(&app).await?;

    let trace = Trace::default();
    let app = start(builder(&trace, None)).await?;
    app.load_deferred_providers().await?;
    app.load_deferred_providers().await?;
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    assert!(!app.is_deferred_service(&named()));
    assert!(!app.is_deferred_service(&ServiceKey::typed::<DeferredValue>()));
    close(&app).await
}

struct CycleValue<const N: u8>;
struct CycleProvider<const N: u8>(Arc<tokio::sync::Barrier>);

impl<const N: u8> DeferrableProvider for CycleProvider<N> {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<CycleValue<N>>()]
    }
}

impl<const N: u8> ServiceProvider for CycleProvider<N> {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.wait().await;
            if N == 1 {
                app.resolve::<CycleValue<2>>().await?;
            } else {
                app.resolve::<CycleValue<1>>().await?;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn independent_tasks_detect_a_cross_provider_wait_cycle() -> Result<()> {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let first = Arc::clone(&barrier);
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || CycleProvider::<1>(Arc::clone(&first)));
    builder.deferred_provider(move || CycleProvider::<2>(Arc::clone(&barrier)));
    let app = start(builder).await?;
    let first = app.state().container().clone();
    let second = app.state().container().clone();
    let mut one = tokio::spawn(async move { first.resolve::<CycleValue<1>>().await });
    let mut two = tokio::spawn(async move { second.resolve::<CycleValue<2>>().await });
    let joined = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(&mut one, &mut two)
    })
    .await;
    let (one_result, two_result) = match joined {
        Ok(results) => results,
        Err(error) => {
            one.abort();
            two.abort();
            let _ = tokio::join!(one, two);
            return Err(Error::internal(error));
        }
    };
    for result in [
        one_result.map_err(Error::internal)?.map(|_| ()),
        two_result.map_err(Error::internal)?.map(|_| ()),
    ] {
        assert!(matches!(result, Err(error) if error.code() == "deferred_provider_cycle"));
    }
    close(&app).await
}

struct CancelledValue;
struct CancelledProvider(Arc<Notify>);

impl DeferrableProvider for CancelledProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<CancelledValue>()]
    }
}

impl ServiceProvider for CancelledProvider {
    fn register<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancelled_registration_notifies_waiters_and_flush_rejects_only_while_in_flight()
-> Result<()> {
    let entered = Arc::new(Notify::new());
    let observed = Arc::clone(&entered);
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || CancelledProvider(Arc::clone(&observed)));
    let mut app = start(builder).await?;
    let container = app.state().container().clone();
    let leader_container = container.clone();
    let leader = tokio::spawn(async move { leader_container.resolve::<CancelledValue>().await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;
    let mut waiter = Box::pin(container.resolve::<CancelledValue>());
    let pending = tokio::time::timeout(Duration::from_millis(20), &mut waiter).await;
    assert!(pending.is_err(), "等待者必须加入仍在运行的注册尝试");
    let flush = app.flush();
    assert!(matches!(flush, Err(error) if error.code() == "deferred_provider_in_progress"));
    leader.abort();
    let cancelled = leader.await;
    assert!(matches!(cancelled, Err(error) if error.is_cancelled()));
    let result = tokio::time::timeout(Duration::from_secs(2), &mut waiter)
        .await
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "deferred_provider_interrupted"));
    drop(waiter);
    app.flush()?;
    assert!(!app.is_deferred_service(&ServiceKey::typed::<CancelledValue>()));
    app.load_deferred_providers().await?;
    close(&app).await
}

#[tokio::test]
async fn scope_close_releases_deferred_waiter_and_cancels_its_own_registration() -> Result<()> {
    let entered = Arc::new(Notify::new());
    let observed = Arc::clone(&entered);
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || CancelledProvider(Arc::clone(&observed)));
    let app = start(builder).await?;
    let container = app.state().container();
    let first_scope = Arc::new(container.scope()?);
    let second_scope = container.scope()?;
    let leader_scope = Arc::clone(&first_scope);
    let leader = tokio::spawn(async move { leader_scope.resolve::<CancelledValue>().await });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(Error::internal)?;

    let mut waiter = Box::pin(second_scope.resolve::<CancelledValue>());
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut waiter)
            .await
            .is_err()
    );
    second_scope.close();
    let result = tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
    assert!(!leader.is_finished());

    first_scope.close();
    let result = tokio::time::timeout(Duration::from_secs(2), leader)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
    close(&app).await
}

struct LateValue;
struct LateProvider {
    trace: Trace,
    pause: Arc<(Notify, Notify)>,
}

impl DeferrableProvider for LateProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<LateValue>()]
    }
}

impl ServiceProvider for LateProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("late.register");
            self.pause.0.notify_one();
            self.pause.1.notified().await;
            app.instance(Arc::new(LateValue)).await
        })
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.trace.push("late.boot");
            app.resolve::<LateValue>().await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn registration_finishing_after_startup_boots_before_resolving() -> Result<()> {
    let trace = Trace::default();
    let pause = Arc::new((Notify::new(), Notify::new()));
    let factory_pause = Arc::clone(&pause);
    let factory_trace = trace.clone();
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || LateProvider {
        trace: factory_trace.clone(),
        pause: Arc::clone(&factory_pause),
    });
    let startup_pause = Arc::clone(&pause);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Mutex::new(Some(sender));
    builder.booting(move |app| {
        let startup_pause = Arc::clone(&startup_pause);
        let sender = sender.lock().unwrap_or_else(PoisonError::into_inner).take();
        Box::pin(async move {
            let container = app.container.clone();
            let task = tokio::spawn(async move { container.resolve::<LateValue>().await });
            if let Some(sender) = sender {
                let _ = sender.send(task);
            }
            tokio::time::timeout(Duration::from_secs(2), startup_pause.0.notified())
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    });
    let app = start(builder).await?;
    assert_eq!(trace.values(), ["late.register"]);
    pause.1.notify_one();
    let task = receiver.await.map_err(Error::internal)?;
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)??;
    assert_eq!(trace.values(), ["late.register", "late.boot"]);
    close(&app).await
}

struct ProbeEvent;

impl crate::events::Event for ProbeEvent {
    const NAME: &'static str = "tests.deferred.event";
}

struct EagerEventTrigger(Trace);

impl ServiceProvider for EagerEventTrigger {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.push("eager.register.begin");
            assert!(!app.is_bound::<DeferredValue>());
            let events = app.resolve::<crate::events::Dispatcher>().await?;
            assert_provider_reply(&events.until(&ProbeEvent).await?)?;
            self.0.push("eager.register.end");
            Ok(())
        })
    }
}

fn assert_provider_reply(
    reply: &crate::support::DataValue,
) -> Result<&Arc<dyn crate::support::DataObject>> {
    let crate::support::DataValue::Object(provider) = reply else {
        return Err(Error::custom(
            "fixture_missing_provider",
            "事件应返回已登记的 Provider",
        ));
    };
    let registered = provider
        .as_any()
        .downcast_ref::<crate::foundation::RegisteredProvider>()
        .ok_or_else(|| Error::custom("fixture_wrong_provider", "事件响应类型不匹配"))?;
    assert_eq!(
        registered.id(),
        crate::foundation::ProviderId::of::<DeferredProbe>()
    );
    assert_eq!(
        registered.provider().map(ServiceProvider::id),
        Some(registered.id())
    );
    Ok(provider)
}

#[tokio::test]
async fn eager_registration_event_registers_before_key_publication_and_boots_once() -> Result<()> {
    let trace = Trace::default();
    let mut builder = builder(&trace, None);
    builder.provider(EagerEventTrigger(trace.clone()));
    let app = start(builder).await?;
    assert_eq!(
        trace.values(),
        [
            "eager.register.begin",
            "lazy.register",
            "eager.register.end",
            "lazy.boot"
        ]
    );
    assert!(app.is_deferred_service(&ServiceKey::typed::<DeferredValue>()));
    assert!(app.is_deferred_service(&named()));
    assert_eq!(app.state().resolve::<DeferredValue>().await?.0, 7);
    close(&app).await
}

#[tokio::test]
async fn event_exact_wildcard_and_until_reuse_original_provider_object() -> Result<()> {
    use crate::events::{Dispatcher, EventsServiceProvider};
    use crate::support::DataValue;

    let trace = Trace::default();
    let mut builder = builder(&trace, None);
    let mut listeners = EventsServiceProvider::new();
    let exact = trace.clone();
    listeners.listen_named_reply("tests.deferred.event", move |_, _| {
        exact.push("normal.exact");
        Box::pin(async { Ok(DataValue::Bool(true)) })
    });
    let wildcard = trace.clone();
    listeners.listen_named_reply("tests.deferred.*", move |_, _| {
        wildcard.push("normal.wildcard");
        Box::pin(async { Ok(DataValue::Null) })
    });
    builder.provider(listeners);
    let app = start(builder).await?;
    let events = app.state().resolve::<Dispatcher>().await?;
    let first = events.until(&ProbeEvent).await?;
    let first = assert_provider_reply(&first)?;
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    let responses = events
        .dispatch(&ProbeEvent)
        .await?
        .ok_or_else(|| Error::custom("fixture_missing_responses", "派发应返回立即响应"))?;
    assert_eq!(responses.len(), 4);
    assert_provider_reply(&responses[0])?;
    assert!(matches!(responses[1], DataValue::Bool(true)));
    let second = assert_provider_reply(&responses[2])?;
    assert!(matches!(responses[3], DataValue::Null));
    assert!(Arc::ptr_eq(first, second));
    assert_eq!(
        trace.values(),
        [
            "lazy.register",
            "lazy.boot",
            "normal.exact",
            "normal.wildcard"
        ]
    );
    assert!(app.is_deferred_service(&ServiceKey::typed::<DeferredValue>()));
    app.load_deferred_providers().await?;
    assert_provider_reply(&events.until(&ProbeEvent).await?)?;
    close(&app).await
}

#[tokio::test]
async fn event_and_service_resolution_wait_for_one_registration() -> Result<()> {
    use crate::events::Dispatcher;

    let trace = Trace::default();
    let pause = Arc::new((Notify::new(), Notify::new()));
    let app = start(builder(&trace, Some(Arc::clone(&pause)))).await?;
    let events = app
        .state()
        .resolve_key::<Dispatcher>(ServiceKey::named("events"))
        .await?;
    assert!(Arc::ptr_eq(
        &events,
        &app.state().resolve::<Dispatcher>().await?
    ));
    let leader = tokio::spawn(async move { events.until(&ProbeEvent).await });
    tokio::time::timeout(Duration::from_secs(2), pause.0.notified())
        .await
        .map_err(Error::internal)?;
    let mut waiter = Box::pin(app.state().resolve::<DeferredValue>());
    let pending = tokio::time::timeout(Duration::from_millis(20), &mut waiter).await;
    pause.1.notify_one();
    assert!(pending.is_err());
    assert_provider_reply(&leader.await.map_err(Error::internal)??)?;
    assert_eq!(waiter.await?.0, 7);
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    assert!(app.is_deferred_service(&ServiceKey::typed::<DeferredValue>()));
    close(&app).await
}

struct EventOnlyProvider(Trace);

impl DeferrableProvider for EventOnlyProvider {
    fn provides() -> Vec<ServiceKey> {
        Vec::new()
    }

    fn when() -> Vec<&'static str> {
        vec!["tests.event-only"]
    }
}

impl ServiceProvider for EventOnlyProvider {
    fn register<'a>(&'a self, _: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let first = self.0.values().is_empty();
            self.0.push("event-only.register");
            if first {
                return Err(Error::custom(
                    "event_only_first_failure",
                    "事件首轮注册失败",
                ));
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn event_only_provider_survives_load_all_and_retries_after_failed_registration() -> Result<()>
{
    use crate::events::{Dispatcher, EventsServiceProvider};
    use crate::support::DataValue;

    let trace = Trace::default();
    let factory_trace = trace.clone();
    let mut builder = ProviderBuilder::new();
    builder.deferred_provider(move || EventOnlyProvider(factory_trace.clone()));
    let mut listeners = EventsServiceProvider::new();
    let observed = trace.clone();
    listeners.listen_named_reply("tests.event-only", move |_, _| {
        observed.push("event-only.listener");
        Box::pin(async { Ok(DataValue::Bool(false)) })
    });
    builder.provider(listeners);
    let mut app = start(builder).await?;
    app.load_deferred_providers().await?;
    assert_eq!(trace.values(), [] as [&str; 0]);
    let events = app.state().resolve::<Dispatcher>().await?;
    let first = events.dispatch_named("tests.event-only", &()).await;
    assert!(matches!(first, Err(error) if error.code() == "event_only_first_failure"));
    assert_eq!(trace.values(), ["event-only.register"]);
    let second = events.dispatch_named("tests.event-only", &()).await?;
    let Some([DataValue::Object(before_flush)]) = second.as_deref() else {
        return Err(Error::custom(
            "fixture_missing_provider",
            "事件应返回已登记的 Provider",
        ));
    };
    assert_eq!(
        trace.values(),
        [
            "event-only.register",
            "event-only.register",
            "event-only.listener"
        ]
    );
    app.flush()?;
    app.register_provider(crate::events::EventServiceProvider)
        .await?;
    app.register_provider(EventsServiceProvider::new()).await?;
    let fresh_events = app.state().resolve::<Dispatcher>().await?;
    assert!(matches!(
        fresh_events.until_named("tests.event-only", &()).await?,
        DataValue::Null
    ));
    let after_flush = events.until_named("tests.event-only", &()).await?;
    let DataValue::Object(after_flush) = after_flush else {
        return Err(Error::custom(
            "fixture_missing_provider",
            "旧派发器应重新登记 Provider",
        ));
    };
    assert!(!Arc::ptr_eq(before_flush, &after_flush));
    assert_eq!(trace.values().last(), Some(&"event-only.register"));
    close(&app).await
}

#[tokio::test]
async fn when_does_not_replace_explicitly_removed_event_service() -> Result<()> {
    let trace = Trace::default();
    let mut builder = builder(&trace, None);
    builder.remove_provider(crate::foundation::ProviderId::of::<
        crate::events::EventServiceProvider,
    >());
    let app = start(builder).await?;
    assert!(
        !app.state()
            .container()
            .contains::<crate::events::Dispatcher>()
    );
    assert!(
        !app.state()
            .container()
            .contains_key(&ServiceKey::named("events"))
    );
    assert_eq!(app.state().resolve::<DeferredValue>().await?.0, 7);
    assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
    close(&app).await
}

#[tokio::test]
async fn when_respects_default_event_provider_exclusion_and_replacement() -> Result<()> {
    use crate::events::{Dispatcher, EventServiceProvider};
    use crate::foundation::ProviderId;
    use crate::support::DefaultProviders;

    struct Replacement;
    impl ServiceProvider for Replacement {}

    let id = ProviderId::of::<EventServiceProvider>();
    let selected = || {
        DefaultProviders::empty()
            .merge([Box::new(EventServiceProvider) as Box<dyn ServiceProvider>])
    };
    for defaults in [
        selected().except([id]),
        selected().replace(id, Box::new(Replacement)),
    ] {
        let trace = Trace::default();
        let mut builder = builder(&trace, None);
        builder.default_providers(defaults);
        let app = start(builder).await?;
        assert!(!app.state().container().contains::<Dispatcher>());
        assert!(
            !app.state()
                .container()
                .contains_key(&ServiceKey::named("events"))
        );
        assert_eq!(app.state().resolve::<DeferredValue>().await?.0, 7);
        assert_eq!(trace.values(), ["lazy.register", "lazy.boot"]);
        close(&app).await?;
    }
    Ok(())
}
