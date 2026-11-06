//! 通过公开 Session API 验证属性、flash、身份与可取消 I/O 生命周期。
//!
//! Spy 只模拟测试要求的 store 结果和阻塞点；不代表数据库或 HTTP 验收。
#![cfg(feature = "session")]

#[path = "session_extensions.rs"]
mod session_extensions;
#[path = "session_helpers.rs"]
mod session_helpers;
#[path = "session_source_cases.rs"]
mod session_source_cases;

#[path = "session_native_database.rs"]
mod native_database;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};

use openrust::session::{
    Expiry, Session, SessionId, SessionRecord, SessionStore, SessionWriteMode, SessionWriteOutcome,
};
use openrust::{Error, Result, core::BoxFuture};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use tokio::sync::oneshot;

struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

#[derive(Default)]
struct SpyState {
    records: HashMap<SessionId, SessionRecord>,
    writes: Vec<(SessionRecord, SessionWriteMode)>,
    deletes: Vec<SessionId>,
    loads: usize,
    outcomes: VecDeque<SessionWriteOutcome>,
    load_gate: Option<Gate>,
    write_gate: Option<Gate>,
}

#[derive(Default)]
struct Spy(Mutex<SpyState>);

impl Spy {
    fn state(&self) -> Result<MutexGuard<'_, SpyState>> {
        self.0.lock().map_err(|_| {
            Error::custom(
                "session_test_spy_poisoned",
                "Session test spy is unavailable",
            )
        })
    }

    fn block(&self, write: bool) -> Result<(oneshot::Receiver<()>, oneshot::Sender<()>)> {
        let (entered, observed) = oneshot::channel();
        let (release, blocked) = oneshot::channel();
        let gate = Gate {
            entered,
            release: blocked,
        };
        let mut state = self.state()?;
        if write {
            state.write_gate = Some(gate);
        } else {
            state.load_gate = Some(gate);
        }
        Ok((observed, release))
    }
}

async fn wait(gate: Option<Gate>) -> Result<()> {
    if let Some(gate) = gate {
        gate.entered.send(()).map_err(|()| {
            Error::custom(
                "session_test_gate_closed",
                "Session test observer was closed",
            )
        })?;
        gate.release.await.map_err(Error::internal)?;
    }
    Ok(())
}

impl SessionStore for Spy {
    fn load<'a>(
        &'a self,
        id: &'a SessionId,
        _: usize,
        _: OffsetDateTime,
    ) -> BoxFuture<'a, Result<Option<SessionRecord>>> {
        Box::pin(async move {
            let gate = {
                let mut state = self.state()?;
                state.loads += 1;
                state.load_gate.take()
            };
            wait(gate).await?;
            Ok(self.state()?.records.get(id).cloned())
        })
    }

    fn write<'a>(
        &'a self,
        record: &'a SessionRecord,
        mode: SessionWriteMode,
        _: usize,
    ) -> BoxFuture<'a, Result<SessionWriteOutcome>> {
        Box::pin(async move {
            let gate = {
                let mut state = self.state()?;
                state.writes.push((record.clone(), mode));
                state.write_gate.take()
            };
            wait(gate).await?;
            let mut state = self.state()?;
            let outcome = state.outcomes.pop_front().unwrap_or_else(|| {
                match (mode, state.records.contains_key(&record.id)) {
                    (SessionWriteMode::Insert, true) => SessionWriteOutcome::Conflict,
                    (SessionWriteMode::Update, false) => SessionWriteOutcome::Missing,
                    _ => SessionWriteOutcome::Written,
                }
            });
            if outcome == SessionWriteOutcome::Written {
                state.records.insert(record.id.clone(), record.clone());
            }
            Ok(outcome)
        })
    }

    fn delete<'a>(&'a self, id: &'a SessionId) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut state = self.state()?;
            state.deletes.push(id.clone());
            state.records.remove(id);
            Ok(())
        })
    }
}

fn expiry() -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(2_000_000_000).map_err(Error::internal)
}

fn session(id: Option<SessionId>, spy: &Arc<Spy>) -> Result<Session> {
    Session::new(id, spy.clone(), Some(Expiry::AtDateTime(expiry()?)))
}

fn token_is_generated(value: Option<Value>) -> bool {
    value
        .and_then(|value| {
            value.as_str().map(|value| {
                value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
        .unwrap_or(false)
}

fn error_code<T>(result: Result<T>) -> Option<String> {
    result.err().map(|error| error.code().to_owned())
}

#[tokio::test]
async fn constructor_exposes_id_before_io_and_missing_valid_id_is_preserved() -> Result<()> {
    let spy = Arc::new(Spy::default());
    let new = session(None, &spy)?;
    let id = new.id()?;
    assert_eq!(id.as_str().len(), 40);
    assert!(id.as_str().bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert!(!new.is_started()?);
    assert_eq!(new.token()?, None);
    assert_eq!(spy.state()?.loads, 0);
    let supplied = SessionId::parse(&"A".repeat(40))?;
    let missing = session(Some(supplied.clone()), &spy)?;
    assert!(missing.start().await?);
    assert_eq!(missing.id()?, supplied);
    assert!(missing.is_started()?);
    assert!(token_is_generated(missing.token()?));
    assert!(spy.state()?.writes.is_empty());
    Ok(())
}

#[tokio::test]
async fn start_generates_only_missing_or_null_tokens_and_merges_loaded_attributes() -> Result<()> {
    for token in [
        None,
        Some(Value::Null),
        Some(json!(false)),
        Some(json!(0)),
        Some(json!("")),
    ] {
        let spy = Arc::new(Spy::default());
        let id = SessionId::parse(&"T".repeat(40))?;
        let mut data = json!({"persisted":"store", "overlap":"store"});
        if let Some(value) = token.clone() {
            data["_token"] = value;
        }
        spy.state()?.records.insert(
            id.clone(),
            SessionRecord {
                id: id.clone(),
                data,
                expiry_date: expiry()?,
            },
        );
        let handle = session(Some(id), &spy)?;
        handle
            .put_typed("overlap", "memory")?
            .put_typed("memory", "retained")?;
        handle.start().await?;
        assert_eq!(handle.get_json("overlap")?, Some(json!("store")));
        assert_eq!(handle.get_json("memory")?, Some(json!("retained")));
        if token.as_ref().is_none_or(Value::is_null) {
            assert!(token_is_generated(handle.token()?));
        } else {
            assert_eq!(handle.token()?, token);
        }
    }
    Ok(())
}

#[tokio::test]
async fn flash_crosses_exactly_one_next_request_and_now_expires_on_save() -> Result<()> {
    let spy = Arc::new(Spy::default());
    let first = session(None, &spy)?;
    first.start().await?;
    let id = first.id()?;
    let token = first.token()?;
    first
        .flash_typed("notice", "next")?
        .now_typed("instant", "current")?;
    assert_eq!(first.get_json("instant")?, Some(json!("current")));
    first.save().await?;
    assert!(!first.is_started()?);
    let second = session(Some(id.clone()), &spy)?;
    second.start().await?;
    assert_eq!(second.get_json("notice")?, Some(json!("next")));
    assert_eq!(second.get_json("instant")?, None);
    assert_eq!(second.token()?, token);
    second.save().await?;
    let third = session(Some(id), &spy)?;
    third.start().await?;
    assert_eq!(third.get_json("notice")?, None);
    assert_eq!(third.get_json("instant")?, None);
    assert_eq!(third.token()?, token);
    Ok(())
}

#[tokio::test]
async fn each_explicit_save_ages_flash_including_a_second_save_in_one_request() -> Result<()> {
    let spy = Arc::new(Spy::default());
    let handle = session(None, &spy)?;
    handle.start().await?;
    handle.flash_typed("once", "value")?;
    handle.save().await?;
    assert_eq!(handle.get_json("once")?, Some(json!("value")));
    handle.save().await?;
    assert_eq!(handle.get_json("once")?, None);
    let state = spy.state()?;
    assert_eq!(state.writes.len(), 2);
    assert_eq!(state.writes[0].1, SessionWriteMode::Insert);
    assert_eq!(state.writes[1].1, SessionWriteMode::Update);
    assert_eq!(state.writes[1].0.data["_flash"], json!({"old":[],"new":[]}));
    Ok(())
}

#[tokio::test]
async fn migrate_regenerate_and_invalidate_have_distinct_token_data_and_delete_semantics()
-> Result<()> {
    let spy = Arc::new(Spy::default());
    let handle = session(None, &spy)?;
    handle.start().await?;
    handle.put_typed("user", 7)?.save().await?;
    let initial_id = handle.id()?;
    let initial_token = handle.token()?;
    assert!(handle.migrate(false).await?);
    let migrated = handle.id()?;
    assert_ne!(migrated, initial_id);
    assert_eq!(handle.token()?, initial_token);
    assert_eq!(handle.get_json("user")?, Some(json!(7)));
    assert_eq!(spy.state()?.deletes.len(), 0);
    assert!(spy.state()?.records.contains_key(&initial_id));
    handle.save().await?;
    assert!(handle.migrate(true).await?);
    assert_ne!(handle.id()?, migrated);
    assert_eq!(handle.token()?, initial_token);
    assert!(!spy.state()?.records.contains_key(&migrated));
    handle.save().await?;
    let before_regenerate = handle.id()?;
    assert!(handle.regenerate(true).await?);
    assert_ne!(handle.id()?, before_regenerate);
    assert_ne!(handle.token()?, initial_token);
    assert!(token_is_generated(handle.token()?));
    assert_eq!(handle.get_json("user")?, Some(json!(7)));
    assert!(!spy.state()?.records.contains_key(&before_regenerate));
    handle.save().await?;
    let before_invalidate = handle.id()?;
    assert!(handle.invalidate().await?);
    let invalidated = handle.id()?;
    assert_ne!(invalidated, before_invalidate);
    assert_eq!(handle.all_json()?, json!([]));
    assert_eq!(handle.token()?, None);
    assert!(!spy.state()?.records.contains_key(&before_invalidate));
    handle.start().await?;
    assert_eq!(handle.id()?, invalidated);
    assert!(token_is_generated(handle.token()?));
    Ok(())
}

#[tokio::test]
async fn cancelling_load_or_save_releases_busy_without_claiming_external_write_rollback()
-> Result<()> {
    for write in [false, true] {
        let spy = Arc::new(Spy::default());
        let handle = session(None, &spy)?;
        if write {
            handle.start().await?;
        }
        let (entered, release) = spy.block(write)?;
        let concurrent = handle.clone();
        let task = tokio::spawn(async move {
            if write {
                concurrent.save().await
            } else {
                concurrent.start().await.map(|_| ())
            }
        });
        entered.await.map_err(Error::internal)?;
        assert_eq!(
            error_code(handle.put_typed("busy", true)),
            Some("session_operation_in_progress".into())
        );
        let before = handle.all_json()?;
        assert_eq!(
            error_code(handle.flash_input_typed(json!({"blocked":true}))),
            Some("session_operation_in_progress".into())
        );
        assert_eq!(handle.all_json()?, before);
        task.abort();
        let cancelled = task.await.err().ok_or_else(|| {
            Error::custom(
                "session_test_not_cancelled",
                "Controlled Session task was not cancelled",
            )
        })?;
        assert!(cancelled.is_cancelled());
        drop(release);
        handle.put_typed("after_abort", "works")?;
        handle.flash_input_typed(json!({"after_abort":true}))?;
        assert_eq!(handle.get_json("after_abort")?, Some(json!("works")));
        // 此 Spy 在闸门之后才写内存；这里只证明 guard 回收，不推断外部 I/O 回滚。
        if write {
            handle.save().await?;
        } else {
            handle.start().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn revoked_update_cannot_resurrect_authenticated_data_under_a_new_insert() -> Result<()> {
    let spy = Arc::new(Spy::default());
    let handle = session(None, &spy)?;
    handle.start().await?;
    handle
        .put_typed("openrust.auth.user_id", 17)?
        .save()
        .await?;
    let id = handle.id()?;
    let request = session(Some(id.clone()), &spy)?;
    request.start().await?;
    spy.state()?.records.remove(&id);
    assert_eq!(
        error_code(request.save().await),
        Some("session_store_write_conflict".into())
    );
    let writes = spy.state()?.writes.len();
    assert_eq!(
        error_code(request.migrate(false).await),
        Some("session_store_write_conflict".into())
    );
    assert_eq!(
        error_code(request.regenerate(true).await),
        Some("session_store_write_conflict".into())
    );
    assert_eq!(
        error_code(request.put_typed("other", true)),
        Some("session_store_write_conflict".into())
    );
    assert_eq!(
        error_code(request.save().await),
        Some("session_store_write_conflict".into())
    );
    assert_eq!(request.id()?, id);
    let state = spy.state()?;
    assert_eq!(state.writes.len(), writes);
    assert_eq!(
        state.writes.last().map(|(_, mode)| *mode),
        Some(SessionWriteMode::Update)
    );
    assert!(state.records.is_empty());
    Ok(())
}

#[tokio::test]
async fn insert_collisions_retry_with_fresh_ids_and_stop_after_three_attempts() -> Result<()> {
    for success in [false, true] {
        let spy = Arc::new(Spy::default());
        let handle = session(None, &spy)?;
        handle.start().await?;
        handle.put_typed("fixed", "preserved")?;
        let mut outcomes = vec![SessionWriteOutcome::Conflict; 2];
        outcomes.push(if success {
            SessionWriteOutcome::Written
        } else {
            SessionWriteOutcome::Conflict
        });
        spy.state()?.outcomes = outcomes.into();
        let saved = handle.save().await;
        if success {
            saved?;
        } else {
            assert_eq!(
                error_code(saved),
                Some("session_store_write_conflict".into())
            );
        }
        {
            let state = spy.state()?;
            assert_eq!(state.writes.len(), 3);
            assert!(
                state
                    .writes
                    .iter()
                    .all(|(record, mode)| *mode == SessionWriteMode::Insert
                        && record.data["fixed"] == json!("preserved"))
            );
            assert_ne!(state.writes[0].0.id, state.writes[1].0.id);
            assert_ne!(state.writes[1].0.id, state.writes[2].0.id);
        }
        if !success {
            assert_eq!(
                error_code(handle.save().await),
                Some("session_store_write_conflict".into())
            );
            assert_eq!(spy.state()?.writes.len(), 3);
        }
    }
    Ok(())
}

#[test]
fn expiry_variants_and_typed_reading_use_public_contracts() -> Result<()> {
    let spy = Arc::new(Spy::default());
    let handle = session(None, &spy)?;
    handle
        .put_typed("answer", 42)?
        .put_typed("null", Value::Null)?;
    assert_eq!(handle.get_typed::<i64>("answer")?, Some(42));
    assert_eq!(handle.get_typed::<i64>("missing")?, None);
    assert_eq!(
        handle
            .get_or(
                "null",
                openrust::support::DataValue::from_json(json!("fallback"))?
            )?
            .to_json()?,
        Value::Null
    );
    let reentrant = handle.get_or_else("missing", || {
        handle.all().unwrap_or(openrust::support::DataValue::Null)
    })?;
    assert_eq!(reentrant.to_json()?, handle.all_json()?);
    assert_eq!(handle.expiry_date()?, expiry()?);
    for strategy in [
        Expiry::OnSessionEnd,
        Expiry::OnInactivity(Duration::minutes(5)),
    ] {
        let before = OffsetDateTime::now_utc();
        handle.set_expiry(Some(strategy))?;
        let actual = handle.expiry_date()?;
        let expected = match strategy {
            Expiry::OnSessionEnd => Duration::minutes(120),
            _ => Duration::minutes(5),
        };
        let after = OffsetDateTime::now_utc();
        assert!(actual >= before + expected && actual <= after + expected);
    }
    Ok(())
}

#[tokio::test]
async fn repeated_start_cannot_turn_deleted_authenticated_record_into_insert()
-> openrust::Result<()> {
    use openrust::session::{MemorySessionStore, Session, SessionStore};
    let store = std::sync::Arc::new(MemorySessionStore::default());
    let original = Session::new(None, store.clone(), None)?;
    original.start().await?;
    original.put_typed("auth.user_id", 42)?;
    original.save().await?;
    let id = original.id()?;
    let stale = Session::new(Some(id.clone()), store.clone(), None)?;
    stale.start().await?;
    assert_eq!(stale.get_typed::<i64>("auth.user_id")?, Some(42));
    store.delete(&id).await?;
    assert_eq!(
        stale
            .start()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("session_store_write_conflict".to_owned())
    );
    assert_eq!(
        stale
            .save()
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("session_store_write_conflict".to_owned())
    );
    assert!(
        store
            .load(
                &id,
                openrust::session::DEFAULT_MAX_BYTES,
                stale.expiry_date()?
            )
            .await?
            .is_none()
    );
    Ok(())
}
