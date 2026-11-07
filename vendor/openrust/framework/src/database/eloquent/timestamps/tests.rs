use super::*;
use crate::support::{CalendarClock, CalendarInstant, ResolvedTimezone};
use serde_json::{Map, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct CountingClock(AtomicUsize);

impl CalendarClock for CountingClock {
    fn now(&self) -> Result<CalendarInstant> {
        self.0.fetch_add(1, Ordering::Relaxed);
        CalendarInstant::new(0, 123_456)
    }
}

#[tokio::test]
async fn timestamp_updates_use_one_instant_and_preserve_manually_changed_columns() -> Result<()> {
    let mut model = ModelMetadata::new("records", "example::Record");
    let clock = Arc::new(CountingClock::default());
    let calendar = CalendarContext::new(ResolvedTimezone::utc(), clock.clone());
    assert!(model.uses_timestamps());
    let new_record = ModelAttributes::default();
    new_record
        .update_timestamps(&model, false, &calendar)
        .await?;
    assert_eq!(clock.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        new_record.raw_attribute("updated_at")?,
        Some(json!("1970-01-01 00:00:00"))
    );
    assert_eq!(
        new_record.raw_attribute("created_at")?,
        new_record.raw_attribute("updated_at")?
    );

    let existing = ModelAttributes::new(Map::from_iter([
        ("created_at".into(), json!("original")),
        ("updated_at".into(), json!("original")),
    ]));
    existing.set_raw_attribute("updated_at", Value::Null)?;
    existing.update_timestamps(&model, true, &calendar).await?;
    assert_eq!(existing.raw_attribute("updated_at")?, Some(Value::Null));
    assert_eq!(
        existing.raw_attribute("created_at")?,
        Some(json!("original"))
    );

    model.created_at_column = None;
    model.updated_at_column = Some("changed_on".into());
    model.date_format = Some("U.u".into());
    let custom = ModelAttributes::default();
    custom.update_timestamps(&model, false, &calendar).await?;
    assert_eq!(
        custom.raw_attributes()?,
        Map::from_iter([("changed_on".into(), json!("0.123456"))])
    );
    Ok(())
}

#[tokio::test]
async fn timestamp_suppression_is_nested_and_model_specific() {
    let model = ModelMetadata::new("records", "example::Record");
    let other = ModelMetadata::new("posts", "example::Post");
    assert!(model.uses_timestamps());
    without_timestamps_on(&["example::Record"], async {
        assert!(!model.uses_timestamps());
        assert!(other.uses_timestamps());
        without_timestamps_on(&["example::Post"], async {
            assert!(!model.uses_timestamps());
            assert!(!other.uses_timestamps());
        })
        .await;
        assert!(other.uses_timestamps());
    })
    .await;
    assert!(model.uses_timestamps());
}

#[tokio::test]
async fn timestamp_suppression_does_not_escape_a_pending_or_cancelled_future() {
    use std::{
        future::{Future, pending, poll_fn},
        pin::pin,
        task::Poll,
    };
    let model = ModelMetadata::new("records", "example::Record");
    {
        let mut suppressed = pin!(without_timestamps_on(&["example::Record"], async {
            assert!(!model.uses_timestamps());
            pending::<()>().await;
        }));
        poll_fn(|cx| {
            assert!(suppressed.as_mut().poll(cx).is_pending());
            assert!(model.uses_timestamps());
            Poll::Ready(())
        })
        .await;
    }
    assert!(model.uses_timestamps());
}
