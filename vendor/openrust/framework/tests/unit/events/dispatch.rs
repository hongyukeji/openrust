use std::sync::{Arc, Mutex};

use super::{Dispatcher, EventServiceProvider, EventsServiceProvider};
use crate::container::ServiceKey;
use crate::foundation::ProviderBuilder;
use figment::Figment;

#[derive(Debug)]
struct Probe(u8);
impl super::Event for Probe {
    const NAME: &'static str = "Tests\\Events\\Probe";
}

async fn events(provider: EventsServiceProvider) -> crate::Result<Dispatcher> {
    let mut builder = ProviderBuilder::new();
    builder.provider(EventServiceProvider).provider(provider);
    let runtime = builder.start(Figment::new()).await?;
    let typed = runtime.container().resolve::<Dispatcher>().await?;
    let named = runtime
        .container()
        .resolve_key::<Dispatcher>(ServiceKey::named("events"))
        .await?;
    assert!(Arc::ptr_eq(&typed, &named));
    Ok((*named).clone())
}

#[tokio::test]
async fn listeners_run_in_order_and_stop_after_the_first_error() -> crate::Result<()> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut plugin = EventsServiceProvider::new();
    let first = Arc::clone(&calls);
    plugin.listen::<Probe, _>(move |event, _| {
        let calls = Arc::clone(&first);
        Box::pin(async move {
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.0);
            Err(crate::Error::custom(
                "rejected",
                "probe listener rejected the event",
            ))
        })
    });
    let second = Arc::clone(&calls);
    plugin.listen::<Probe, _>(move |event, _| {
        let calls = Arc::clone(&second);
        Box::pin(async move {
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event.0 + 1);
            Ok(())
        })
    });
    let dispatcher = events(plugin).await?;
    assert!(dispatcher.dispatch(&Probe(4)).await.is_err());
    assert_eq!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_slice(),
        [4]
    );
    Ok(())
}

#[tokio::test]
async fn unrelated_event_types_are_not_delivered() -> crate::Result<()> {
    #[derive(Debug)]
    struct Other;
    impl super::Event for Other {
        const NAME: &'static str = "Tests\\Events\\Other";
    }
    let calls = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&calls);
    let mut plugin = EventsServiceProvider::new();
    plugin.listen::<Probe, _>(move |_, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            *observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            Ok(())
        })
    });
    events(plugin).await?.dispatch(&Other).await?;
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        0
    );
    Ok(())
}
#[test]
fn named_patterns_match_full_case_sensitive_names_and_only_star_is_special() {
    for (pattern, name, expected) in [
        ("modules.*.enabled", "modules.blog.enabled", true),
        ("modules.*.enabled", "modules.a.b.enabled", true),
        ("modules.*.enabled", "prefix.modules.blog.enabled", false),
        ("Modules.*", "modules.blog.enabled", false),
        ("modules.?.enabled", "modules.a.enabled", false),
        ("modules.?.enabled", "modules.?.enabled", true),
        ("*", "", true),
        ("a**b", "ab", true),
        ("é*", "éß", true),
        ("a*b", "ac", false),
        ("", "x", false),
    ] {
        assert_eq!(
            super::wildcard_name_matches(pattern, name),
            expected,
            "{pattern:?} / {name:?}"
        );
    }
}

#[tokio::test]
async fn canonical_and_string_share_identity_but_other_names_do_not() -> crate::Result<()> {
    use super::{Event, EventPropagation};
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut provider = EventsServiceProvider::new();
    let typed = Arc::clone(&calls);
    provider.listen::<Probe, _>(move |event, context| {
        let typed = Arc::clone(&typed);
        Box::pin(async move {
            typed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(("typed", event.0, context.event_name().to_owned()));
            Ok(())
        })
    });
    let named = Arc::clone(&calls);
    provider.listen_named::<Probe, _>("modules.*.enabled", move |event, context| {
        let named = Arc::clone(&named);
        Box::pin(async move {
            named
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(("named", event.0, context.event_name().to_owned()));
            Ok(EventPropagation::Continue)
        })
    });
    let events = events(provider).await?;
    events.dispatch(&Probe(1)).await?;
    events
        .dispatch_named("modules.blog.disabled", &Probe(2))
        .await?;
    events
        .dispatch_named("modules.blog.enabled", &Probe(3))
        .await?;
    events.dispatch_named(Probe::NAME, &Probe(4)).await?;
    assert_eq!(
        *calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [
            ("typed", 1, Probe::NAME.to_owned()),
            ("named", 3, "modules.blog.enabled".to_owned()),
            ("typed", 4, Probe::NAME.to_owned())
        ]
    );
    Ok(())
}
