//! 固定 Laravel 源码推导的三个 Session 候选行为，经公开内存 store 与 Session API 回归。
//!
//! 语料标为 `source_candidate`，未执行真实 PHP Store oracle；本测试不把候选称为 golden。
#![cfg(feature = "session")]

use openrust::session::{
    DEFAULT_MAX_BYTES, Expiry, MemorySessionStore, Session, SessionId, SessionRecord, SessionStore,
    SessionWriteMode, SessionWriteOutcome,
};
use openrust::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

#[derive(Deserialize)]
struct Corpus {
    evidence_status: String,
    oracle_executed: bool,
    cases: Vec<SourceCase>,
}

#[derive(Deserialize)]
struct SourceCase {
    id: String,
    evidence_status: String,
    setup: Setup,
    operations: Vec<Value>,
    expected_final_attributes: Value,
}

#[derive(Deserialize)]
struct Setup {
    handler_record_attributes: Value,
    valid_id: String,
}

fn mismatch() -> Error {
    Error::custom(
        "test_session_source_case_mismatch",
        "Session 源码候选行为不匹配",
    )
}

fn ensure(condition: bool) -> Result<()> {
    if condition { Ok(()) } else { Err(mismatch()) }
}

fn source_case(id: &str, operations: &Value) -> Result<SourceCase> {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/laravel-session/source-cases.json"))
            .map_err(Error::internal)?;
    ensure(corpus.evidence_status == "source_candidate" && !corpus.oracle_executed)?;
    let case = corpus
        .cases
        .into_iter()
        .find(|case| case.id == id)
        .ok_or_else(mismatch)?;
    ensure(case.evidence_status == "source_candidate")?;
    ensure(&Value::Array(case.operations.clone()) == operations)?;
    Ok(case)
}

async fn seeded(
    case: &SourceCase,
) -> Result<(Session, Arc<MemorySessionStore>, SessionId, OffsetDateTime)> {
    let store = Arc::new(MemorySessionStore::default());
    let id = SessionId::parse(&case.setup.valid_id)?;
    let expiry = OffsetDateTime::now_utc()
        .checked_add(Duration::days(1))
        .ok_or_else(mismatch)?;
    let record = SessionRecord {
        id: id.clone(),
        data: case.setup.handler_record_attributes.clone(),
        expiry_date: expiry,
    };
    ensure(
        store
            .write(&record, SessionWriteMode::Insert, DEFAULT_MAX_BYTES)
            .await?
            == SessionWriteOutcome::Written,
    )?;
    let session = Session::new(
        Some(id.clone()),
        store.clone(),
        Some(Expiry::AtDateTime(expiry)),
    )?;
    ensure(session.start().await?)?;
    Ok((session, store, id, expiry))
}

async fn save_and_reopen(
    session: &Session,
    store: Arc<MemorySessionStore>,
    id: SessionId,
    expiry: OffsetDateTime,
    expected: &Value,
) -> Result<Session> {
    session.save().await?;
    ensure(session.all()? == *expected)?;
    let next = Session::new(Some(id), store, Some(Expiry::AtDateTime(expiry)))?;
    ensure(next.start().await? && next.all()? == *expected)?;
    Ok(next)
}

fn aged_without_flash(mut attributes: Value) -> Result<Value> {
    let Some(root) = attributes.as_object_mut() else {
        return Err(mismatch());
    };
    root.insert("_flash".into(), json!({"new":[],"old":[]}));
    Ok(attributes)
}

#[tokio::test]
async fn forget_empty_list_keeps_attributes_and_persists() -> Result<()> {
    let case = source_case(
        "forget_empty_list_is_noop",
        &json!([
            {"op":"start","args":[],"expected_return":true},
            {"op":"forget","args":[[]]}
        ]),
    )?;
    let (session, store, id, expiry) = seeded(&case).await?;
    session.forget(&[])?;
    ensure(session.all()? == case.expected_final_attributes)?;
    ensure(session.get("one")? == Some(json!(1)))?;
    let expected = aged_without_flash(case.expected_final_attributes)?;
    let next = save_and_reopen(&session, store, id, expiry, &expected).await?;
    ensure(next.get("one")? == Some(json!(1)))
}

#[tokio::test]
async fn forget_missing_nested_key_keeps_scalar_parent_and_persists() -> Result<()> {
    let case = source_case(
        "forget_missing_nested_keeps_scalar_parent",
        &json!([
            {"op":"start","args":[],"expected_return":true},
            {"op":"forget","args":["a.b"]}
        ]),
    )?;
    let (session, store, id, expiry) = seeded(&case).await?;
    session.forget(&["a.b"])?;
    ensure(session.all()? == case.expected_final_attributes)?;
    ensure(session.get("a")? == Some(json!("scalar")) && session.get("a.b")?.is_none())?;
    let expected = aged_without_flash(case.expected_final_attributes)?;
    let next = save_and_reopen(&session, store, id, expiry, &expected).await?;
    ensure(next.get("a")? == Some(json!("scalar")) && next.get("a.b")?.is_none())
}

#[tokio::test]
async fn forget_flash_value_keeps_tracking_through_save_and_next_request() -> Result<()> {
    let case = source_case(
        "forget_flash_value_does_not_implicitly_clear_tracking",
        &json!([
            {"op":"start","args":[],"expected_return":true},
            {"op":"flash","args":["x","value"]},
            {"op":"forget","args":["x"]},
            {"op":"get","args":["_flash.new"],"expected_return":["x"]},
            {"op":"save","args":[]},
            {"op":"next_request","args":[]},
            {"op":"exists","args":["x"],"expected_return":false}
        ]),
    )?;
    let (session, store, id, expiry) = seeded(&case).await?;
    session.flash("x", "value")?;
    ensure(session.get("x")? == Some(json!("value")))?;
    session.forget(&["x"])?;
    ensure(session.get("x")?.is_none() && session.get("_flash.new")? == Some(json!(["x"])))?;
    let next =
        save_and_reopen(&session, store, id, expiry, &case.expected_final_attributes).await?;
    ensure(!next.exists(&["x"])? && next.get("_flash.old")? == Some(json!(["x"])))
}
