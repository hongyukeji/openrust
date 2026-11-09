//! 外部包真实事件与应用类共用一个 exact/wildcard/响应管线。
use openrust::{
    Result,
    events::{Dispatcher, Event, EventsServiceProvider},
    foundation::ProviderBuilder,
    support::DataValue,
};
use openrust_translation_database::TranslationSourceChanged;
use std::sync::{Arc, Mutex};

struct ApplicationEvent(u8);
impl Event for ApplicationEvent {
    const NAME: &'static str = "Consumer\\ApplicationEvent";
}

#[tokio::test]
async fn external_event_and_local_class_share_one_heterogeneous_listener() -> Result<()> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut provider = EventsServiceProvider::new();
    let observed = Arc::clone(&log);
    provider.listen_named_reply("*", move |payload, context| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            let value = if let Some(local) = payload.downcast_ref::<ApplicationEvent>() {
                u64::from(local.0)
            } else {
                payload
                    .downcast_ref::<TranslationSourceChanged>()
                    .ok_or(openrust::Error::Unauthorized)?
                    .revision
            };
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((context.event_name().to_owned(), value));
            Ok(DataValue::String("observed".into()))
        })
    });
    let local = ApplicationEvent(4);
    let pointer = std::ptr::from_ref(&local).addr();
    provider.listen::<ApplicationEvent, _>(move |event, context| {
        assert_eq!(std::ptr::from_ref(event).addr(), pointer);
        assert_eq!(context.event_name(), ApplicationEvent::NAME);
        Box::pin(async { Ok(()) })
    });
    provider.listen_named_reply(ApplicationEvent::NAME, move |payload, _| {
        assert_eq!(
            payload
                .downcast_ref::<ApplicationEvent>()
                .map(|v| std::ptr::from_ref(v).addr()),
            Some(pointer)
        );
        Box::pin(async { Ok(DataValue::Number(0.into())) })
    });
    let mut builder = ProviderBuilder::new();
    builder
        .provider(openrust::events::EventServiceProvider)
        .provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let events=runtime.container().resolve::<Dispatcher>().await?;
        for replies in [events.dispatch(&local).await?,events.dispatch_named(ApplicationEvent::NAME,&local).await?] {
            assert!(matches!(replies.as_deref(),Some([DataValue::Null,DataValue::Number(n),DataValue::String(s)]) if n.as_u64()==Some(0) && s=="observed"));
        }
        assert!(matches!(events.until(&local).await?,DataValue::Number(n) if n.as_i64()==Some(0)));
        events.dispatch(&TranslationSourceChanged { revision:9 }).await?;
        assert_eq!(*log.lock().unwrap_or_else(std::sync::PoisonError::into_inner),[
            (ApplicationEvent::NAME.to_owned(),4),(ApplicationEvent::NAME.to_owned(),4),(TranslationSourceChanged::NAME.to_owned(),9)]);
        Ok(())
    }.await;
    let cleanup = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await;
    outcome.and(cleanup)
}
