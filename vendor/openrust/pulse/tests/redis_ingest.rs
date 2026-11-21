use openrust_pulse::{
    Entry, Item, PulseFuture,
    ingests::{Ingest, RedisIngest, Retention},
    storage::{Graph, Storage},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default)]
struct Sink {
    items: Mutex<Vec<Item>>,
    fail: AtomicBool,
}
impl Storage for Sink {
    fn values<'a>(
        &'a self,
        _: &'a str,
        _: Option<&'a [String]>,
    ) -> PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(async { unreachable!() })
    }
    fn graph<'a>(&'a self, _: &'a [String], _: &'a str, _: i64) -> PulseFuture<'a, Graph> {
        Box::pin(async { unreachable!() })
    }
    fn aggregate<'a>(
        &'a self,
        _: &'a str,
        _: &'a [String],
        _: i64,
        _: Option<&'a str>,
        _: &'a str,
        _: u64,
    ) -> PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(async { unreachable!() })
    }
    fn aggregateTypes<'a>(
        &'a self,
        _: &'a [String],
        _: &'a str,
        _: i64,
        _: Option<&'a str>,
        _: &'a str,
        _: u64,
    ) -> PulseFuture<'a, Vec<openrust::database::Row>> {
        Box::pin(async { unreachable!() })
    }
    fn aggregateTotal<'a>(
        &'a self,
        _: &'a [String],
        _: &'a str,
        _: i64,
    ) -> PulseFuture<'a, indexmap::IndexMap<String, Option<f64>>> {
        Box::pin(async { unreachable!() })
    }
    fn store<'a>(&'a self, items: &'a [Item]) -> PulseFuture<'a, ()> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(openrust_pulse::Error::StateUnavailable);
            }
            self.items.lock().unwrap().extend_from_slice(items);
            Ok(())
        })
    }
    fn trim(&self) -> PulseFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn purge<'a>(&'a self, _: Option<&'a [String]>) -> PulseFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test]
#[ignore = "requires isolated Redis 8 fixture and OPENRUST_TEST_REDIS_URL"]
async fn redis_stream_batches_retry_without_deleting_failed_storage_and_trim_retention() {
    let prefix = format!("pulse-test-{:032x}:", fastrand::u128(..));
    let config=openrust::config::Repository::new([("database".into(),json!({"redis":{"options":{"prefix":prefix},"default":{"url":std::env::var("OPENRUST_TEST_REDIS_URL").expect("isolated Redis fixture URL")}}}))].into());
    let connection = openrust::redis::RedisManager::new(Arc::new(config))
        .connection(None)
        .unwrap();
    let key = connection.key("openrust:pulse:ingest");
    let ingest = RedisIngest::new(connection.clone()).withChunk(2).unwrap();
    let sink = Sink::default();
    let now = jiff::Timestamp::now().as_second();
    let items: Vec<Item> = (0..5)
        .map(|i| {
            Entry::new(now, "redis", i.to_string(), Some(i))
                .count()
                .onlyBuckets()
                .into()
        })
        .collect();
    ingest.ingest(&items).await.unwrap();
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(5)
    );
    sink.fail.store(true, Ordering::SeqCst);
    assert!(ingest.digest(&sink).await.is_err());
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(5)
    );
    sink.fail.store(false, Ordering::SeqCst);
    assert_eq!(ingest.digest(&sink).await.unwrap(), 5);
    assert_eq!(ingest.digest(&sink).await.unwrap(), 0);
    {
        let stored = sink.items.lock().unwrap();
        assert_eq!(stored.len(), 5);
        for item in stored.iter() {
            let Item::Entry(e) = item else { panic!() };
            assert!(e.isCount());
            assert!(e.isOnlyBuckets());
        }
    }
    let invalid = b"invalid-json";
    connection
        .command("XADD", &[key.as_bytes(), b"*", b"data", invalid])
        .await
        .unwrap();
    assert!(ingest.digest(&sink).await.is_err());
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(1)
    );
    connection.command("DEL", &[key.as_bytes()]).await.unwrap();
    let trim = RedisIngest::new(connection.clone()).withKeep(Retention::Entries(0));
    trim.ingest(&items).await.unwrap();
    trim.trim().await.unwrap();
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(0)
    );
    let trim = RedisIngest::new(connection.clone()).withKeep(Retention::Seconds(0));
    trim.ingest(&items).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    trim.trim().await.unwrap();
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(0)
    );
    connection.command("DEL", &[key.as_bytes()]).await.unwrap();
}
