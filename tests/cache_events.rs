use std::sync::{Arc, Mutex};

use openrust::{
    cache::{CacheManager, Ttl, Value, events::*},
    contracts::{
        cache::Repository as _,
        container::Container as _,
        events::{Dispatcher as _, Event, Listener},
        foundation::Application as _,
    },
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    testing::TestApp,
};

struct Record(Arc<Mutex<Vec<String>>>);

impl Listener for Record {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        self.0.lock().unwrap().push(event.name().into());
        Box::pin(async { Ok(None) })
    }
}

fn configure(
    builder: openrust::foundation::configuration::ApplicationBuilder,
) -> openrust::foundation::configuration::ApplicationBuilder {
    builder.withConfig(|_, _| {
        Ok([(
            "cache".into(),
            serde_json::json!({
                "default": "array",
                "stores": {
                    "array": {"driver": "array"},
                    "quiet": {"driver": "array", "events": false},
                    "failover": {"driver": "failover", "stores": ["array"]},
                    "loud": {"driver": "failover", "stores": ["array"], "events": true}
                }
            }),
        )]
        .into())
    })
}

#[tokio::test]
async fn cache_events_follow_operations_and_artisan_clear_order() {
    let mut app = TestApp::create(configure).unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.app().container().make::<Dispatcher>().unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    dispatcher
        .listen("*", Arc::new(Record(log.clone())), Delivery::Sync)
        .unwrap();
    let cache = app
        .app()
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();

    assert_eq!(cache.get("missing").await.unwrap(), None);
    assert!(
        cache
            .putMany(
                &[
                    ("key".into(), 3.into()),
                    ("null".into(), Value::from(serde_json::Value::Null))
                ],
                Some(Ttl::Seconds(60))
            )
            .await
            .unwrap()
    );
    assert_eq!(
        cache.many(&["key", "null", "missing"]).await.unwrap(),
        [
            Some(3.into()),
            Some(Value::from(serde_json::Value::Null)),
            None
        ]
    );
    assert!(!cache.forget("missing").await.unwrap());
    assert!(
        cache
            .put("key", &4.into(), Some(Ttl::Seconds(0)))
            .await
            .unwrap()
    );
    let result = app.artisan(["cache:clear", "array", "--locks"]).await;
    assert_eq!(result.status, 0, "{}", result.stderr);
    let result = app
        .artisan(["cache:clear", "array", "--tags=reports"])
        .await;
    assert_eq!(result.status, 0, "{}", result.stderr);

    let names: Vec<_> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|name| {
            [
                "RetrievingKey",
                "CacheMissed",
                "WritingManyKeys",
                "KeyWritten",
                "RetrievingManyKeys",
                "CacheHit",
                "ForgettingKey",
                "KeyForgetFailed",
                "KeyForgotten",
                "CacheLocksFlushing",
                "CacheLocksFlushed",
                "cache:clearing",
                "CacheFlushing",
                "CacheFlushed",
                "cache:cleared",
            ]
            .contains(&name.as_str())
        })
        .cloned()
        .collect();
    assert_eq!(
        names,
        [
            "RetrievingKey",
            "CacheMissed",
            "WritingManyKeys",
            "KeyWritten",
            "KeyWritten",
            "RetrievingManyKeys",
            "CacheHit",
            "CacheMissed",
            "CacheMissed",
            "ForgettingKey",
            "KeyForgetFailed",
            "ForgettingKey",
            "KeyForgotten",
            "CacheLocksFlushing",
            "CacheLocksFlushed",
            "cache:clearing",
            "CacheFlushing",
            "CacheFlushed",
            "cache:cleared"
        ]
    );
}

#[tokio::test]
async fn cache_event_payloads_follow_tags_opt_out_memo_and_failover() {
    let mut app = TestApp::create(configure).unwrap();
    let events = app.fakeEvents(&[]).await.unwrap();
    let manager = app.app().container().make::<CacheManager>().unwrap();
    let cache = manager.store(None).unwrap();
    let quiet = manager.store(Some("quiet")).unwrap();
    assert!(quiet.forever("private", &"secret".into()).await.unwrap());
    events.assertNotDispatched(WritingKey::NAME);

    let tags = cache.tags(&["reports", "reports", "daily"]).unwrap();
    assert!(
        tags.put("latest", &"ready".into(), Some(Ttl::Seconds(60)))
            .await
            .unwrap()
    );
    assert_eq!(tags.get("latest").await.unwrap(), Some("ready".into()));
    let writes = events.dispatched(WritingKey::NAME);
    let write = writes[0].downcast_ref::<WritingKey>().unwrap();
    assert_eq!(write.storeName.as_deref(), Some("array"));
    assert_eq!(write.key, "latest");
    assert_eq!(write.tags, ["reports", "reports", "daily"]);
    assert_eq!(write.seconds, Some(60));
    assert_eq!(write.value, Value::from("ready"));
    assert!(tags.flush().await.unwrap());
    let flushes = events.dispatched(CacheFlushed::NAME);
    assert_eq!(
        flushes[0].downcast_ref::<CacheFlushed>().unwrap().tags,
        ["reports", "reports", "daily"]
    );

    let memo = manager.memo(None).unwrap();
    assert_eq!(memo.get("memo").await.unwrap(), None);
    assert_eq!(memo.get("memo").await.unwrap(), None);
    let failover = manager.store(Some("failover")).unwrap();
    assert_eq!(failover.get("fail").await.unwrap(), None);
    let loud = manager.store(Some("loud")).unwrap();
    assert_eq!(loud.get("loud").await.unwrap(), None);
    let misses = events.dispatched(CacheMissed::NAME);
    let observed: Vec<_> = misses
        .iter()
        .map(|event| {
            let miss = event.downcast_ref::<CacheMissed>().unwrap();
            (miss.storeName.as_deref().unwrap(), miss.key.as_str())
        })
        .collect();
    assert_eq!(
        observed,
        [
            ("array", "memo"),
            ("array", "fail"),
            ("array", "loud"),
            ("loud", "loud")
        ]
    );

    let mut other = TestApp::create(configure).unwrap();
    let other_events = other.fakeEvents(&[CacheMissed::NAME]).await.unwrap();
    assert_eq!(cache.get("private").await.unwrap(), None);
    other_events.assertNothingDispatched();
}

struct Reject;

impl Listener for Reject {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async { Err(openrust::events::EventError::InvalidPayload) })
    }
}

#[tokio::test]
async fn cache_listener_errors_preserve_mutation_order() {
    let mut app = TestApp::create(configure).unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.app().container().make::<Dispatcher>().unwrap();
    let cache = app
        .app()
        .container()
        .make::<CacheManager>()
        .unwrap()
        .store(None)
        .unwrap();
    dispatcher
        .listen(WritingKey::NAME, Arc::new(Reject), Delivery::Sync)
        .unwrap();
    assert!(cache.forever("blocked", &1.into()).await.is_err());
    assert_eq!(cache.get("blocked").await.unwrap(), None);

    dispatcher.forget(WritingKey::NAME);
    dispatcher
        .listen(KeyWritten::NAME, Arc::new(Reject), Delivery::Sync)
        .unwrap();
    assert!(cache.forever("stored", &2.into()).await.is_err());
    assert_eq!(cache.get("stored").await.unwrap(), Some(2.into()));

    dispatcher.forget(KeyWritten::NAME);
    assert!(
        cache
            .forever("reports/latest", &"ready".into())
            .await
            .unwrap()
    );
    let events = app.fakeEvents(&[CacheHit::NAME]).await.unwrap();
    let _ = cache.get("reports/latest").await.unwrap();
    events.assertDispatched(CacheHit::NAME);
}
