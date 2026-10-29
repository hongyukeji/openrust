//! Store 便捷 API 的公开行为；内存 driver 仅证明同请求状态，不替代双库验收。
#![cfg(feature = "session")]

use openrust::session::{MemorySessionStore, Session, SessionStore};
use openrust::{Error, Result};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn session() -> Result<Session> {
    Session::new(None, Arc::new(MemorySessionStore::default()), None)
}

#[test]
fn remember_distinguishes_null_and_false_and_preserves_reentrant_side_effects() -> Result<()> {
    let session = session()?;
    let calls = AtomicUsize::new(0);
    for (key, value) in [
        ("false", json!(false)),
        ("zero", json!(0)),
        ("empty", json!("")),
    ] {
        session.put(key, value.clone())?;
        assert_eq!(
            session.remember(key, || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok("wrong")
            })?,
            value
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    session.put("null", Value::Null)?;
    assert_eq!(
        session.remember("null", || {
            session.put("other", "kept")?.put("null", "temporary")?;
            Ok("final")
        })?,
        json!("final")
    );
    assert_eq!(session.get("null")?, Some(json!("final")));
    assert_eq!(session.get("other")?, Some(json!("kept")));
    let failure = session.remember::<Value>("failure", || {
        session.put("callback_side_effect", true)?;
        Err(Error::custom("callback_failed", "受控回调失败"))
    });
    assert_eq!(
        failure.err().map(|error| error.code().to_owned()),
        Some("callback_failed".into())
    );
    assert_eq!(session.get("failure")?, None);
    assert_eq!(session.get("callback_side_effect")?, Some(json!(true)));
    Ok(())
}

struct ReentrantValue<'a> {
    session: &'a Session,
    calls: &'a AtomicUsize,
    fail: bool,
}
impl Serialize for ReentrantValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.session
            .put("serializer_side_effect", true)
            .map_err(serde::ser::Error::custom)?;
        if self.fail {
            return Err(serde::ser::Error::custom("受控序列化失败"));
        }
        serializer.serialize_i64(7)
    }
}

#[test]
fn helpers_serialize_once_without_holding_state_lock_and_keep_failure_side_effects() -> Result<()> {
    let session = session()?;
    let calls = AtomicUsize::new(0);
    assert_eq!(
        session.remember("computed", || Ok(ReentrantValue {
            session: &session,
            calls: &calls,
            fail: false
        }))?,
        json!(7)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(session.get("serializer_side_effect")?, Some(json!(true)));
    session.push(
        "list",
        ReentrantValue {
            session: &session,
            calls: &calls,
            fail: false,
        },
    )?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(session.get("list")?, Some(json!([7])));
    session.put("stable", "original")?;
    assert!(
        session
            .push(
                "stable",
                ReentrantValue {
                    session: &session,
                    calls: &calls,
                    fail: true
                }
            )
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(session.get("stable")?, Some(json!("original")));
    assert_eq!(session.get("serializer_side_effect")?, Some(json!(true)));
    Ok(())
}

#[test]
fn push_preserves_sparse_php_array_keys_and_rejects_non_arrays_without_writing() -> Result<()> {
    let session = session()?;
    for (key, initial) in [("null", Value::Null), ("false", json!(false))] {
        session.put(key, initial)?.push(key, "new")?;
        assert_eq!(session.get(key)?, Some(json!(["new"])));
    }
    session.push("missing", "new")?;
    assert_eq!(session.get("missing")?, Some(json!(["new"])));
    for (initial, expected) in [
        (
            json!({"2":"two","5":"five"}),
            json!({"2":"two","5":"five","6":"new"}),
        ),
        (
            json!({"-5":"a","-2":"b"}),
            json!({"-5":"a","-2":"b","-1":"new"}),
        ),
        (json!({"label":"old"}), json!({"label":"old","0":"new"})),
    ] {
        session.put("sparse", initial)?.push("sparse", "new")?;
        assert_eq!(session.get("sparse")?, Some(expected));
    }
    for value in [json!(true), json!(3), json!("abc")] {
        session.put("invalid", value.clone())?;
        assert!(session.push("invalid", "new").is_err());
        assert_eq!(session.get("invalid")?, Some(value));
    }
    Ok(())
}

#[test]
fn arithmetic_uses_php_numbers_left_union_and_rejects_warnings_without_writing() -> Result<()> {
    let session = session()?;
    assert_eq!(session.increment("missing")?, json!(1));
    assert_eq!(session.decrement("missing")?, json!(0));
    for (value, expected) in [
        (Value::Null, json!(1)),
        (json!(false), json!(1)),
        (json!(true), json!(2)),
        (json!("42"), json!(43)),
        (json!(" 2.5 "), json!(3.5)),
        (json!("1e3"), json!(1001.0)),
    ] {
        session.put("number", value)?;
        assert_eq!(session.increment("number")?, expected);
    }
    for value in [json!("42tail"), json!(""), json!("tail"), json!([1])] {
        session.put("number", value.clone())?;
        assert!(session.increment("number").is_err());
        assert_eq!(session.get("number")?, Some(value));
    }
    session.put("union", json!({"a":1,"2":"left"}))?;
    assert_eq!(
        session.increment_by("union", json!({"a":9,"2":"right","b":2}))?,
        json!({"a":1,"2":"left","b":2})
    );
    let before = session.get("union")?;
    assert!(session.decrement_by("union", json!({"c":3})).is_err());
    assert_eq!(session.get("union")?, before);
    session.put("maximum", i64::MAX)?;
    assert_eq!(
        session.increment("maximum")?,
        json!(9.223_372_036_854_776e18)
    );
    session.put("minimum", i64::MIN)?;
    let intermediate = session.decrement_by("minimum", i64::MIN)?;
    assert!(intermediate.is_f64());
    assert_eq!(intermediate.as_f64(), Some(0.0));
    session.put("finite", f64::MAX)?;
    assert!(session.increment_by("finite", f64::MAX).is_err());
    assert_eq!(session.get("finite")?, Some(json!(f64::MAX)));
    Ok(())
}

#[test]
fn helper_record_budget_failure_publishes_no_partial_mutation_and_remove_keeps_null() -> Result<()>
{
    let session = session()?;
    session.put("keep", true)?.put("null", Value::Null)?;
    assert_eq!(session.remove("absent")?, None);
    assert_eq!(session.remove("null")?, Some(Value::Null));
    let before = session.all()?;
    let oversized = "x".repeat(openrust::session::DEFAULT_MAX_BYTES + 1);
    assert!(session.push("oversized", &oversized).is_err());
    assert_eq!(session.all()?, before);
    assert!(session.remember("oversized", || Ok(&oversized)).is_err());
    assert_eq!(session.all()?, before);
    Ok(())
}

#[tokio::test]
async fn handler_is_the_same_arc_and_external_revocation_still_rejects_managed_updates()
-> Result<()> {
    let store: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::default());
    let session = Session::new(None, store.clone(), None)?;
    assert!(Arc::ptr_eq(&store, &session.handler()?));
    session.start().await?;
    session.put("counter", 2)?.save().await?;
    session.handler()?.delete(&session.id()?).await?;
    assert!(session.save().await.is_err());
    let before = session.all()?;
    assert!(session.increment("counter").is_err());
    assert!(session.push("list", "x").is_err());
    assert!(session.remove("counter").is_err());
    assert!(session.set_name("changed").is_err());
    let called = AtomicUsize::new(0);
    assert!(
        session
            .remember("missing", || {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(1)
            })
            .is_err()
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert_eq!(session.all()?, before);
    Ok(())
}

struct PendingStore;
impl SessionStore for PendingStore {
    fn load<'a>(
        &'a self,
        _: &'a openrust::session::SessionId,
        _: usize,
        _: time::OffsetDateTime,
    ) -> openrust::core::BoxFuture<'a, Result<Option<openrust::session::SessionRecord>>> {
        Box::pin(std::future::pending())
    }
    fn write<'a>(
        &'a self,
        _: &'a openrust::session::SessionRecord,
        _: openrust::session::SessionWriteMode,
        _: usize,
    ) -> openrust::core::BoxFuture<'a, Result<openrust::session::SessionWriteOutcome>> {
        Box::pin(std::future::pending())
    }
    fn delete<'a>(
        &'a self,
        _: &'a openrust::session::SessionId,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn busy_helpers_reject_before_callbacks_and_cancellation_releases_gate() -> Result<()> {
    let session = Session::new(None, Arc::new(PendingStore), None)?;
    session.put("counter", 1)?;
    let mut start = Box::pin(session.start());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), start.as_mut())
            .await
            .is_err()
    );
    let calls = AtomicUsize::new(0);
    assert_eq!(
        session
            .remember("absent", || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(1)
            })
            .err()
            .map(|error| error.code().to_owned()),
        Some("session_operation_in_progress".into())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(session.increment("counter").is_err());
    assert!(session.decrement("counter").is_err());
    assert!(session.push("list", 1).is_err());
    assert!(session.remove("counter").is_err());
    assert!(session.set_name("new_name").is_err());
    assert_eq!(session.get("counter")?, Some(json!(1)));
    drop(start);
    assert_eq!(session.increment("counter")?, json!(2));
    Ok(())
}

#[test]
fn persisted_php_expression_and_official_handler_cases_match_public_helpers() -> Result<()> {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/laravel-session-d2/golden.json"))
            .map_err(Error::internal)?;
    assert_eq!(fixture["php_version"], "8.5.0");
    let cases = fixture["cases"]
        .as_array()
        .ok_or_else(|| Error::custom("session_fixture_invalid", "差分case不是数组"))?;
    let mut executed = 0;
    for case in cases
        .iter()
        .filter(|case| case["mode"] == "official_handle_error")
    {
        let session = session()?;
        session.put("value", &case["initial"])?;
        let result = match case["op"].as_str() {
            Some("push") => session
                .push("value", &case["amount"])
                .and_then(|session| session.get("value"))
                .map(|value| value.unwrap_or(Value::Null)),
            Some("increment") => session.increment_by("value", &case["amount"]),
            Some("decrement") => session.decrement_by("value", &case["amount"]),
            _ => return Err(Error::custom("session_fixture_invalid", "未知差分运算")),
        };
        match case["outcome"].as_str() {
            Some("value") => {
                let actual = result?;
                if case["type"] == "float" {
                    assert!(actual.is_f64(), "case: {}", case["case"]);
                    assert_eq!(
                        actual.as_f64(),
                        case["value"].as_f64(),
                        "case: {}",
                        case["case"]
                    );
                } else {
                    assert_eq!(actual, case["value"], "case: {}", case["case"]);
                }
            }
            Some("error" | "nonfinite") => {
                assert!(result.is_err(), "case: {}", case["case"]);
                assert_eq!(
                    session.get("value")?,
                    Some(case["initial"].clone()),
                    "case: {}",
                    case["case"]
                );
            }
            _ => return Err(Error::custom("session_fixture_invalid", "未知差分结果")),
        }
        executed += 1;
    }
    assert_eq!(executed, 22);
    Ok(())
}

#[test]
fn set_name_rejects_invalid_and_over_budget_cookie_names_atomically() -> Result<()> {
    use openrust::http::{CookieFactory, CookieOptions};
    let session = session()?;
    session.set_name("valid_session")?;
    let factory = CookieFactory::default();
    for name in ["", "0", "bad\u{1}name"] {
        assert!(
            factory
                .make(name, "", 0, CookieOptions::default())
                .and_then(|cookie| cookie.to_header(false))
                .is_err()
        );
        assert!(session.set_name(name).is_err());
        assert_eq!(session.name()?, "valid_session");
    }
    // Cookie header 预算是私有实现细节；通过公开工厂定位失败大小，不绑定内部常量。
    let mut length = 1024;
    loop {
        let name = "x".repeat(length);
        if factory
            .make(name.clone(), "", 0, CookieOptions::default())
            .and_then(|cookie| cookie.to_header(false))
            .is_err()
        {
            assert!(session.set_name(name).is_err());
            assert_eq!(session.name()?, "valid_session");
            break;
        }
        length *= 2;
        assert!(
            length <= 1024 * 1024,
            "公开Cookie预算必须在测试资源上限内拒绝"
        );
    }
    Ok(())
}
