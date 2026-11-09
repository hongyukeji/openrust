struct SingletonProbe;
impl super::Event for SingletonProbe {
    const NAME: &'static str = "Tests\\SingletonProbe";
}
use super::*;
use crate::foundation::{Application, ProviderBuilder};
use crate::support::ServiceProvider;
use figment::Figment;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test]
async fn provider_shares_dispatcher_and_preserves_order_and_short_circuit() -> Result<()> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut provider = EventsServiceProvider::new();
    for index in 0..3 {
        let calls = Arc::clone(&calls);
        provider.listen::<SingletonProbe, _>(move |_, _| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(index);
                if index == 1 {
                    Err(Error::custom("listener_rejected", "rejected"))
                } else {
                    Ok(())
                }
            })
        });
    }
    let mut builder = ProviderBuilder::new();
    builder.provider(EventServiceProvider).provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let first = runtime.container().resolve::<Dispatcher>().await?;
    let second = runtime.container().resolve::<Dispatcher>().await?;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        first
            .dispatch(&SingletonProbe)
            .await
            .err()
            .map(|error| error.code().to_owned())
            .as_deref(),
        Some("listener_rejected")
    );
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![0, 1]
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn core_dispatcher_is_lazy_and_uses_one_named_and_typed_instance() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder.provider(EventServiceProvider);
    let runtime = builder.start(Figment::new()).await?;
    let container = runtime.container();
    assert!(container.get_ready::<Dispatcher>().is_none());
    let named = container
        .resolve_key::<Dispatcher>(ServiceKey::named("events"))
        .await?;
    assert!(Arc::ptr_eq(
        &named,
        &container.resolve::<Dispatcher>().await?
    ));
    assert!(named.selected(SingletonProbe::NAME, false)?.is_empty());
    assert!(matches!(
        named.until(&SingletonProbe).await?,
        DataValue::Null
    ));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}

struct ExistingDispatcher(Arc<Dispatcher>);

impl ServiceProvider for ExistingDispatcher {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let named = ServiceKey::named("events");
            app.instance_key(named.clone(), Arc::clone(&self.0)).await?;
            app.alias(named, ServiceKey::typed::<Dispatcher>())
        })
    }
}

#[tokio::test]
async fn core_preserves_an_application_dispatcher_aliased_to_the_named_key() -> Result<()> {
    let expected = Arc::new(Dispatcher::new(DeferredEventHooks::default()));
    let mut builder = ProviderBuilder::new();
    builder
        .provider(ExistingDispatcher(Arc::clone(&expected)))
        .provider(EventServiceProvider);
    let runtime = builder.start(Figment::new()).await?;
    let container = runtime.container();
    let named = container
        .resolve_key::<Dispatcher>(ServiceKey::named("events"))
        .await?;
    assert!(Arc::ptr_eq(&named, &expected));
    assert!(Arc::ptr_eq(
        &named,
        &container.resolve::<Dispatcher>().await?
    ));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}

fn reply_hook(label: &'static str) -> DeferredEventHook {
    Arc::new(move || Box::pin(async move { Ok(DataValue::String(label.to_owned())) }))
}

#[tokio::test]
async fn regular_listeners_keep_their_deferred_order_and_reject_late_old_publication() -> Result<()>
{
    const NAME: &str = "test.provider-order";
    let directory: Arc<DeferredEventListeners> = Arc::default();
    let dispatcher = Dispatcher::new(directory.hooks());
    assert!(
        !directory
            .replace(ProviderId::of::<u8>(), &[NAME], &reply_hook("first"))
            .dispose()
    );
    let stale = dispatcher.provider_hooks.reserve_regular_position();
    let current = dispatcher.provider_hooks.reserve_regular_position();
    assert!(
        !directory
            .replace(ProviderId::of::<u16>(), &[NAME], &reply_hook("last"))
            .dispose()
    );
    for (position, label) in [(current, "regular"), (stale, "unexpected")] {
        dispatcher.publish_regular(
            RegularListeners {
                position,
                listeners: vec![Arc::new(ProviderListener {
                    selector: NAME,
                    hook: reply_hook(label),
                })],
            },
            &HashMap::new(),
        )?;
    }

    assert!(!dispatcher.selected(NAME, false)?.is_empty());
    assert!(matches!(
        dispatcher.dispatch_named(NAME, &()).await?.as_deref(),
        Some([DataValue::String(first), DataValue::String(regular), DataValue::String(last)])
            if first == "first" && regular == "regular" && last == "last"
    ));
    Ok(())
}

struct ConflictingProbe;

impl Event for ConflictingProbe {
    const NAME: &'static str = SingletonProbe::NAME;
}

fn replies(value: bool) -> EventsServiceProvider {
    let mut provider = EventsServiceProvider::new();
    provider.listen_reply::<SingletonProbe, _>(move |_, _| {
        Box::pin(async move { Ok(DataValue::Bool(value)) })
    });
    provider
}

#[tokio::test]
async fn a_foreign_dispatcher_cannot_consume_another_applications_listener_registration()
-> Result<()> {
    let mut original = ProviderBuilder::new();
    original
        .provider(EventServiceProvider)
        .provider(replies(true));
    let runtime = original.start(Figment::new()).await?;
    let dispatcher = runtime.container().resolve::<Dispatcher>().await?;

    let mut other = ProviderBuilder::new();
    other
        .provider(ExistingDispatcher(Arc::clone(&dispatcher)))
        .provider(EventServiceProvider)
        .provider(replies(false));
    let result = other.start(Figment::new()).await;
    assert!(
        matches!(result, Err(error) if error.code() == "event_dispatcher_application_mismatch")
    );
    assert!(matches!(
        dispatcher.until(&SingletonProbe).await?,
        DataValue::Bool(true)
    ));
    runtime
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await
}

#[tokio::test]
async fn early_dispatcher_clone_observes_listener_publication_and_safe_live_replacement()
-> Result<()> {
    let early = Arc::new(std::sync::OnceLock::new());
    let mut builder = ProviderBuilder::new();
    builder
        .provider(EventServiceProvider)
        .provider(replies(true));
    builder.configure_after_register({
        let early = Arc::clone(&early);
        move |container| {
            Box::pin(async move {
                let dispatcher = container.resolve::<Dispatcher>().await?;
                assert!(dispatcher.selected(SingletonProbe::NAME, false)?.is_empty());
                assert!(early.set((*dispatcher).clone()).is_ok());
                Ok(())
            })
        }
    });
    let runtime = builder.start(Figment::new()).await?;
    let mut app = Application::from_provider_runtime(runtime)?;
    let shared = app.state().resolve::<Dispatcher>().await?;
    let early = early
        .get()
        .ok_or_else(|| Error::custom("test", "未取得提前解析的派发器"))?;
    assert!(!early.selected(SingletonProbe::NAME, false)?.is_empty());
    assert!(matches!(
        early.until(&SingletonProbe).await?,
        DataValue::Bool(true)
    ));

    app.force_register_provider(replies(false)).await?;
    let named = app
        .state()
        .resolve_key::<Dispatcher>(ServiceKey::named("events"))
        .await?;
    assert!(Arc::ptr_eq(&named, &shared));
    assert!(matches!(
        early.until(&SingletonProbe).await?,
        DataValue::Bool(false)
    ));
    let mut invalid = EventsServiceProvider::new();
    invalid.listen::<ConflictingProbe, _>(|_, _| Box::pin(async { Ok(()) }));
    let error = app.force_register_provider(invalid).await;
    assert!(matches!(error, Err(error) if error.code() == "event_name_conflict"));
    assert!(matches!(
        early.until(&SingletonProbe).await?,
        DataValue::Bool(false)
    ));
    app.shutdown(Instant::now() + Duration::from_secs(1)).await
}
