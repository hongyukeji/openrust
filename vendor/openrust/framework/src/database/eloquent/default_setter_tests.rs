//! 调用级默认setter的优先级、借用、冲突及取消反证；复用真实属性状态。

use super::{AttributeSetter, ModelAttributes};
use crate::{
    Error, Result,
    database::eloquent::{AttributeCast, ModelMetadata},
    support::BoxFuture,
};
use serde_json::{Map, Value, json};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Semaphore;

fn metadata(cast: Option<AttributeCast>) -> ModelMetadata {
    let mut metadata = ModelMetadata::new("records", "test::Record");
    if let Some(cast) = cast {
        metadata.casts.insert("secret".into(), cast);
    }
    metadata
}
fn initial() -> ModelAttributes {
    ModelAttributes::new(Map::from_iter([("secret".into(), json!("original"))]))
}
fn failure() -> Error {
    Error::custom("default_setter_test_failure", "默认setter测试失败")
}
fn send_future<F: Future + Send>(future: F) -> F {
    future
}

struct RecordingSetter {
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl AttributeSetter for RecordingSetter {
    fn set(
        &self,
        value: Value,
        snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(snapshot.get("secret"), Some(&json!("original")));
            tokio::task::yield_now().await;
            if self.fail {
                return Err(failure());
            }
            Ok(Map::from_iter([
                ("secret".into(), value),
                ("derived".into(), json!(true)),
            ]))
        })
    }
}
struct BorrowedSetter<'a>(&'a dyn AttributeSetter);
impl AttributeSetter for BorrowedSetter<'_> {
    fn set(
        &self,
        value: Value,
        snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        self.0.set(value, snapshot)
    }
}

#[tokio::test]
async fn registered_custom_success_or_failure_never_invokes_default_or_cast() -> Result<()> {
    for fail in [false, true] {
        let store = initial();
        let custom_calls = Arc::new(AtomicUsize::new(0));
        let default_calls = Arc::new(AtomicUsize::new(0));
        store.register_setter(
            "secret",
            Arc::new(RecordingSetter {
                calls: Arc::clone(&custom_calls),
                fail,
            }),
        )?;
        let default = RecordingSetter {
            calls: Arc::clone(&default_calls),
            fail: false,
        };
        // 无HashManager上下文且Hashed声明：两种mutator都必须在自动cast之前选择。
        let result = store
            .set_attribute_with_default_setter(
                &metadata(Some(AttributeCast::Hashed)),
                "secret",
                json!({"raw": 1}),
                &default,
            )
            .await;
        if fail {
            assert!(matches!(result, Err(error) if error.code() == "default_setter_test_failure"));
            assert_eq!(store.raw_attribute("secret")?, Some(json!("original")));
            assert_eq!(store.raw_attribute("derived")?, None);
        } else {
            result?;
            assert_eq!(store.raw_attribute("secret")?, Some(json!({"raw": 1})));
            assert_eq!(store.raw_attribute("derived")?, Some(json!(true)));
        }
        assert_eq!(custom_calls.load(Ordering::SeqCst), 1);
        assert_eq!(default_calls.load(Ordering::SeqCst), 0);
        assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 0);
    }
    Ok(())
}

#[tokio::test]
async fn borrowed_default_is_send_merges_multiple_fields_and_does_not_recast() -> Result<()> {
    let store = initial();
    let calls = Arc::new(AtomicUsize::new(0));
    let default = RecordingSetter {
        calls: Arc::clone(&calls),
        fail: false,
    };
    let borrowed = BorrowedSetter(&default);
    let object: &dyn AttributeSetter = &borrowed;
    let metadata = metadata(Some(AttributeCast::Array));
    send_future(store.set_attribute_with_default_setter(
        &metadata,
        "secret",
        json!({"n": 1}),
        object,
    ))
    .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(json!({"n": 1})));
    assert_eq!(store.raw_attribute("derived")?, Some(json!(true)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        store
            .state
            .read()
            .map_err(|_| failure())?
            .setters
            .is_empty()
    );
    // 原无default入口仍执行Array cast，不把新默认行为变成全局开关。
    store
        .set_attribute(&metadata, "secret", json!({"n": 2}))
        .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(json!("{\"n\":2}")));
    Ok(())
}

#[tokio::test]
async fn default_failure_keeps_external_effect_but_publishes_no_raw_patch() -> Result<()> {
    let store = initial();
    let calls = Arc::new(AtomicUsize::new(0));
    let default = RecordingSetter {
        calls: Arc::clone(&calls),
        fail: true,
    };
    let result = store
        .set_attribute_with_default_setter(&metadata(None), "secret", json!("candidate"), &default)
        .await;
    assert!(matches!(result, Err(error) if error.code() == "default_setter_test_failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.raw_attribute("secret")?, Some(json!("original")));
    assert_eq!(store.raw_attribute("derived")?, None);
    assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 0);
    store.sync_original()?;
    Ok(())
}

struct GatedSetter {
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    calls: Arc<AtomicUsize>,
}
impl AttributeSetter for GatedSetter {
    fn set(
        &self,
        value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| failure())?
                .forget();
            Ok(Map::from_iter([
                ("secret".into(), value),
                ("derived".into(), json!(true)),
            ]))
        })
    }
}
#[derive(Clone, Copy)]
enum Interference {
    Unrelated,
    FieldAba,
    Register,
    AccessorAba,
}

async fn interfered_assignment(interference: Interference) -> Result<()> {
    let store = Arc::new(initial());
    let entered = Arc::new(Semaphore::new(0));
    let release = Arc::new(Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let gated: Arc<dyn AttributeSetter> = Arc::new(GatedSetter {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        calls: Arc::clone(&calls),
    });
    if matches!(interference, Interference::AccessorAba) {
        store.register_setter("secret", Arc::clone(&gated))?;
    }
    let running = Arc::clone(&store);
    let fallback = Arc::clone(&gated);
    let task = tokio::spawn(async move {
        running
            .set_attribute_with_default_setter(
                &metadata(None),
                "secret",
                json!("candidate"),
                fallback.as_ref(),
            )
            .await
    });
    entered.acquire().await.map_err(|_| failure())?.forget();
    match interference {
        Interference::Unrelated => store.set_raw_attribute("other", json!(7))?,
        Interference::FieldAba => {
            store.set_raw_attribute("secret", json!("successor"))?;
            store.set_raw_attribute("secret", json!("original"))?;
        }
        Interference::Register | Interference::AccessorAba => {
            store.register_setter(
                "secret",
                Arc::new(RecordingSetter {
                    calls: Arc::new(AtomicUsize::new(0)),
                    fail: false,
                }),
            )?;
            if matches!(interference, Interference::AccessorAba) {
                store.register_setter("secret", gated)?;
            }
        }
    }
    release.add_permits(1);
    let result = task.await.map_err(Error::internal)?;
    if matches!(interference, Interference::Unrelated) {
        result?;
        assert_eq!(store.raw_attribute("secret")?, Some(json!("candidate")));
        assert_eq!(store.raw_attribute("derived")?, Some(json!(true)));
        assert_eq!(store.raw_attribute("other")?, Some(json!(7)));
    } else {
        assert!(
            matches!(result, Err(error) if error.code() == "model_attribute_mutation_conflict")
        );
        assert_eq!(store.raw_attribute("secret")?, Some(json!("original")));
        assert_eq!(store.raw_attribute("derived")?, None);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 0);
    Ok(())
}

#[tokio::test]
async fn default_watch_keeps_unrelated_writes_and_rejects_field_or_accessor_successors()
-> Result<()> {
    for interference in [
        Interference::Unrelated,
        Interference::FieldAba,
        Interference::Register,
        Interference::AccessorAba,
    ] {
        interfered_assignment(interference).await?;
    }
    Ok(())
}

#[tokio::test]
async fn cancellation_releases_default_watch_without_installing_or_publishing() -> Result<()> {
    let store = Arc::new(initial());
    let entered = Arc::new(Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let running = Arc::clone(&store);
    let default = GatedSetter {
        entered: Arc::clone(&entered),
        release: Arc::new(Semaphore::new(0)),
        calls: Arc::clone(&calls),
    };
    let task = tokio::spawn(async move {
        running
            .set_attribute_with_default_setter(
                &metadata(None),
                "secret",
                json!("candidate"),
                &default,
            )
            .await
    });
    entered.acquire().await.map_err(|_| failure())?.forget();
    assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 1);
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert_eq!(store.raw_attribute("secret")?, Some(json!("original")));
    assert_eq!(store.raw_attribute("derived")?, None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = store.state.read().map_err(|_| failure())?;
    assert_eq!(state.active_setters, 0);
    assert!(state.setters.is_empty());
    drop(state);
    store.sync_original()?;
    Ok(())
}

#[cfg(feature = "hashing")]
struct BorrowedHashingSetter<'a> {
    hashing: &'a crate::hashing::HashManager,
    calls: &'a AtomicUsize,
}
#[cfg(feature = "hashing")]
impl AttributeSetter for BorrowedHashingSetter<'_> {
    fn set(
        &self,
        value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let plain = match &value {
                Value::Null => None,
                Value::String(plain) => Some(plain.as_str()),
                _ => return Err(failure()),
            };
            let encoded = crate::database::eloquent::casts::cast_attribute_as_hashed_string(
                self.hashing,
                plain,
            )
            .await?;
            Ok(Map::from_iter([(
                "secret".into(),
                encoded.map_or(Value::Null, Value::String),
            )]))
        })
    }
}

#[cfg(feature = "hashing")]
#[tokio::test]
async fn invocation_default_borrows_the_same_manager_across_send_future() -> Result<()> {
    let store = initial();
    let hashing = crate::hashing::HashManager::new(crate::hashing::HashingConfig {
        bcrypt: crate::hashing::BcryptConfig {
            rounds: 4,
            verify: true,
            limit: None,
        },
        ..crate::hashing::HashingConfig::default()
    })?;
    let calls = AtomicUsize::new(0);
    let default = BorrowedHashingSetter {
        hashing: &hashing,
        calls: &calls,
    };
    send_future(store.set_attribute_with_default_setter(
        &metadata(Some(AttributeCast::Hashed)),
        "secret",
        json!("public-fixture"),
        &default,
    ))
    .await?;
    let encoded = store.raw_attribute("secret")?.ok_or_else(failure)?;
    let encoded = encoded.as_str().ok_or_else(failure)?;
    assert!(encoded.starts_with("$2y$04$"));
    assert!(
        hashing
            .check(
                "public-fixture",
                Some(encoded),
                &crate::hashing::HashOptions::new()
            )
            .await?
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        store
            .state
            .read()
            .map_err(|_| failure())?
            .setters
            .is_empty()
    );
    Ok(())
}

struct ReentrantDefault<'a>(&'a ModelAttributes);
impl AttributeSetter for ReentrantDefault<'_> {
    fn set(
        &self,
        value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        // 同步进入trait回调时就必须无锁；try_write使错误实现明确失败而非把测试阻死。
        let unlocked = self.0.state.try_write().is_ok();
        Box::pin(async move {
            if !unlocked {
                return Err(failure());
            }
            self.0.set_raw_attribute("other", json!(7))?;
            tokio::task::yield_now().await;
            Ok(Map::from_iter([("secret".into(), value)]))
        })
    }
}

#[tokio::test]
async fn default_invocation_is_unlocked_and_preserves_its_unrelated_reentry() -> Result<()> {
    let store = initial();
    let default = ReentrantDefault(&store);
    send_future(store.set_attribute_with_default_setter(
        &metadata(None),
        "secret",
        json!("candidate"),
        &default,
    ))
    .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(json!("candidate")));
    assert_eq!(store.raw_attribute("other")?, Some(json!(7)));
    assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 0);
    Ok(())
}
