use openrust_pulse::{
    ByteValue, Entry, Item, PulseFuture,
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
    fn aggregate_types<'a>(
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
    fn aggregate_total<'a>(
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
    let ingest = RedisIngest::new(connection.clone()).with_chunk(2).unwrap();
    let sink = Sink::default();
    let now = jiff::Timestamp::now().as_second();
    let mut items: Vec<Item> = (0..5)
        .map(|i| {
            Entry::new(now, "redis", i.to_string(), Some(i))
                .count()
                .only_buckets()
                .into()
        })
        .collect();
    let original = vec![b'X', 0xc3, b'X', 0xa9, b'X'];
    let raw = Entry::from_key_bytes(now, "redis", original.clone(), Some(4))
        .count()
        .only_buckets();
    let raw_value = ByteValue::from_key_bytes(now, "redis", original.clone(), "payload");
    items[3] = raw_value.into();
    items[4] = raw.clone().into();
    let text = serde_json::to_value(&items[0]).unwrap();
    assert_eq!(text["data"]["key"], "0");
    let encoded = serde_json::to_value(&items[4]).unwrap();
    assert_eq!(encoded["data"]["key"], json!(original));
    assert!(serde_json::from_value::<String>(encoded["data"]["key"].clone()).is_err());
    let encoded_value = serde_json::to_value(&items[3]).unwrap();
    assert_eq!(encoded_value["class"], "ByteValue");
    assert_eq!(encoded_value["data"]["key"], json!(original));
    let mut edited = raw;
    edited.key = "edited".into();
    assert_eq!(edited.key_bytes(), b"edited");
    assert_eq!(serde_json::to_value(edited).unwrap()["key"], "edited");
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
        for (index, item) in stored.iter().enumerate() {
            if index == 3 {
                let Item::ByteValue(value) = item else {
                    panic!()
                };
                assert_eq!(value.key, original);
                assert_eq!(value.value, "payload");
                continue;
            }
            let Item::Entry(e) = item else { panic!() };
            if index == 4 {
                assert_eq!(e.key_bytes(), original);
            } else {
                assert_eq!(e.key, index.to_string());
            }
            assert!(e.is_count());
            assert!(e.is_only_buckets());
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
    let trim = RedisIngest::new(connection.clone()).with_keep(Retention::Entries(0));
    trim.ingest(&items).await.unwrap();
    trim.trim().await.unwrap();
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(0)
    );
    let trim = RedisIngest::new(connection.clone()).with_keep(Retention::Seconds(0));
    // 消费者和 Redis 可以处于时钟不同步的主机，显式写入已过期的消息。
    connection.command("DEL", &[key.as_bytes()]).await.unwrap();
    let encoded_value = serde_json::to_vec(&items[3]).unwrap();
    connection
        .command("XADD", &[key.as_bytes(), b"1-0", b"data", &encoded_value])
        .await
        .unwrap();
    trim.trim().await.unwrap();
    assert_eq!(
        connection.command("XLEN", &[key.as_bytes()]).await.unwrap(),
        openrust::redis::Value::Int(0)
    );
    connection.command("DEL", &[key.as_bytes()]).await.unwrap();
}
