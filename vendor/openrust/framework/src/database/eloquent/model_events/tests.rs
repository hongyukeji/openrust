use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Mapped(Arc<AtomicUsize>);

impl Event for Mapped {
    const NAME: &'static str = "ExampleMappedModelEvent";
}

fn mapped<'a>(
    events: &'a Events,
    model: &'a Arc<AtomicUsize>,
    transaction: Option<&'a DatabaseTransaction>,
    halt: bool,
) -> BoxFuture<'a, Result<DataValue>> {
    Box::pin(async move {
        dispatch_mapped_model_event(events, &Mapped(model.clone()), transaction, halt).await
    })
}

#[tokio::test]
async fn mapped_false_cancels_and_empty_mapped_response_falls_back_to_same_model() -> Result<()> {
    let metadata = ModelMetadata::new("records", "example::Record");
    let model = Arc::new(AtomicUsize::new(0));
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen_reply::<Mapped, _>(|event, _| {
        Box::pin(async move { Ok(DataValue::Bool(event.0.load(Ordering::Relaxed) != 0)) })
    });
    let expected = model.clone();
    provider.listen_named_reply("eloquent.saving: example::Record", move |payload, _| {
        let expected = expected.clone();
        Box::pin(async move {
            let model = payload
                .downcast_ref::<Arc<AtomicUsize>>()
                .ok_or_else(|| crate::Error::custom("model_event_payload", "模型事件类型错误"))?;
            assert!(Arc::ptr_eq(&expected, model));
            model.fetch_add(1, Ordering::Relaxed);
            Ok(DataValue::Bool(false))
        })
    });
    let events = test_events(provider).await?;
    assert!(matches!(
        fire_model_event(
            Some(&events),
            &metadata,
            &model,
            "saving",
            true,
            Some(&mapped)
        )
        .await?,
        DataValue::Bool(false)
    ));
    assert_eq!(model.load(Ordering::Relaxed), 0);
    assert!(matches!(
        fire_model_event(Some(&events), &metadata, &model, "saving", true, None).await?,
        DataValue::Bool(false)
    ));
    assert_eq!(model.load(Ordering::Relaxed), 1);
    assert!(matches!(
        fire_model_event(
            Some(&events),
            &metadata,
            &model,
            "saving",
            true,
            Some(&mapped)
        )
        .await?,
        DataValue::Bool(true)
    ));
    assert_eq!(model.load(Ordering::Relaxed), 1);
    Ok(())
}

fn empty_mapped<'a>(
    _events: &'a Events,
    _model: &'a (),
    _transaction: Option<&'a DatabaseTransaction>,
    halt: bool,
) -> BoxFuture<'a, Result<DataValue>> {
    Box::pin(async move {
        Ok(if halt {
            DataValue::from("0")
        } else {
            DataValue::Array(vec![(3_i64.into(), DataValue::Null)])
        })
    })
}

#[tokio::test]
async fn mapped_falsy_scalar_and_null_only_list_allow_named_fallback() -> Result<()> {
    let metadata = ModelMetadata::new("records", "example::Record");
    let mut provider = crate::events::EventsServiceProvider::new();
    provider.listen_named_reply("eloquent.saving: example::Record", |_, _| {
        Box::pin(async { Ok(DataValue::from("named")) })
    });
    provider.listen_named_reply("eloquent.saved: example::Record", |_, _| {
        Box::pin(async { Ok(DataValue::from(9_i64)) })
    });
    let events = test_events(provider).await?;
    let before = fire_model_event(
        Some(&events),
        &metadata,
        &(),
        "saving",
        true,
        Some(&empty_mapped),
    )
    .await?;
    assert!(matches!(before, DataValue::String(value) if value == "named"));
    let after = fire_model_event(
        Some(&events),
        &metadata,
        &(),
        "saved",
        false,
        Some(&empty_mapped),
    )
    .await?;
    assert!(
        matches!(after.direct("0"), Some(DataValue::Number(value)) if value.as_i64() == Some(9))
    );
    assert!(matches!(
        fire_model_event(None, &metadata, &(), "saving", true, None).await?,
        DataValue::Bool(true)
    ));
    Ok(())
}
