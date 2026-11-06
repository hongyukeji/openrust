//! 原值与提交视图隔离、持久变化时点和稳定快照回归。
use super::super::{AttributeGetter, AttributeSetter};
use super::*;
use crate::support::BoxFuture;
use serde_json::json;
use std::sync::Arc;

fn raw(values: &[(&str, Value)]) -> Map<String, Value> {
    values
        .iter()
        .map(|(key, value)| ((*key).into(), value.clone()))
        .collect()
}
fn metadata(casts: &[(&str, AttributeCast)]) -> ModelMetadata {
    let mut metadata = ModelMetadata::new("records", "app::Record");
    metadata.casts = casts
        .iter()
        .map(|(key, cast)| ((*key).into(), *cast))
        .collect();
    metadata
}
fn assert_code<T>(result: Result<T>, code: &str) {
    assert!(matches!(result, Err(error) if error.code() == code));
}

#[test]
fn hydration_original_is_not_committed_mutation_view() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("secret", json!("plain"))]));
    let metadata = metadata(&[("secret", AttributeCast::Hashed)]);
    assert_eq!(store.get_raw_original("secret")?, Some(json!("plain")));
    assert!(store.get_dirty(&metadata)?.is_empty());
    let mut receipt = store
        .set_raw_attribute_reversibly_if("secret", json!("pending"), |_| Ok(true))?
        .ok_or_else(|| persistence_error("fixture_receipt_missing"))?;
    assert!(matches!(
        store.committed_raw_attribute("secret"),
        Err(error) if error.code() == "model_attribute_update_pending"
            && error.status().as_u16() == 409
    ));
    assert_eq!(store.raw_attribute("secret")?, Some(json!("pending")));
    assert_eq!(store.get_raw_original("secret")?, Some(json!("plain")));
    assert_code(store.sync_original(), "model_attribute_update_pending");
    assert_code(
        store.sync_changes(&metadata),
        "model_attribute_update_pending",
    );
    assert_code(
        store.is_clean(&metadata, &[]),
        "model_attribute_update_pending",
    );
    assert!(receipt.rollback()?);
    assert_eq!(
        store.committed_raw_attribute("secret")?,
        Some(json!("plain"))
    );
    store.set_raw_attribute("secret", json!("committed"))?;
    assert_eq!(
        store.committed_raw_attribute("secret")?,
        Some(json!("committed"))
    );
    assert_eq!(store.get_raw_original("secret")?, Some(json!("plain")));
    assert_eq!(
        store.get_dirty(&metadata)?,
        raw(&[("secret", json!("committed"))])
    );
    assert!(
        ModelAttributes::default()
            .get_raw_original_attributes()?
            .is_empty()
    );
    Ok(())
}

#[test]
fn changes_previous_and_clean_have_distinct_publication_times() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("a", json!(1)), ("deleted", json!(9))]));
    let metadata = metadata(&[]);
    store.set_raw_attributes(raw(&[("a", json!(2)), ("new", Value::Null)]))?;
    assert!(!store.was_changed(&[])?);
    store.sync_changes(&metadata)?;
    assert_eq!(
        store.get_changes()?,
        raw(&[("a", json!(2)), ("new", Value::Null)])
    );
    assert_eq!(store.get_previous()?, raw(&[("a", json!(1))]));
    assert!(store.was_changed(&["new"])?);
    assert!(!store.was_changed(&["deleted"])?);
    assert!(store.is_dirty(&metadata, &["a"])?);
    let raw_revision = store.state.read().map_err(|_| state_error())?.revision;
    store.sync_original()?;
    assert!(store.is_clean(&metadata, &[])?);
    assert_eq!(
        store.state.read().map_err(|_| state_error())?.revision,
        raw_revision
    );
    assert!(store.was_changed(&["a"])?);
    assert_eq!(store.get_previous()?, raw(&[("a", json!(1))]));
    store.sync_changes(&metadata)?;
    assert!(store.get_previous()?.is_empty());
    assert!(!store.was_changed(&[])?);
    Ok(())
}

#[test]
fn partial_original_sync_preserves_prefix_and_rejects_capacity_before_write() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("a", json!(1)), ("c", json!(3))]));
    store.set_raw_attributes(raw(&[("a", json!(2)), ("c", json!(4))]))?;
    let before = store
        .state
        .read()
        .map_err(|_| state_error())?
        .original_revision;
    assert_code(
        store.sync_original_attributes(&["missing", "a"]),
        "model_attribute_original_missing",
    );
    assert_eq!(
        store
            .state
            .read()
            .map_err(|_| state_error())?
            .original_revision,
        before
    );
    assert_code(
        store.sync_original_attributes(&["a", "missing", "c"]),
        "model_attribute_original_missing",
    );
    assert_eq!(store.get_raw_original("a")?, Some(json!(2)));
    assert_eq!(store.get_raw_original("c")?, Some(json!(3)));
    assert_eq!(
        store
            .state
            .read()
            .map_err(|_| state_error())?
            .original_revision,
        before + 1
    );
    store.sync_original_attributes(&["c", "c"])?;
    assert_eq!(store.get_raw_original("c")?, Some(json!(4)));
    store.set_raw_attribute("c", json!(5))?;
    store
        .state
        .write()
        .map_err(|_| state_error())?
        .original_revision = u64::MAX;
    assert_code(
        store.sync_original_attributes(&["c"]),
        "model_attribute_original_revision_exhausted",
    );
    assert_eq!(store.get_raw_original("c")?, Some(json!(4)));
    Ok(())
}

#[test]
fn stale_raw_or_original_snapshot_cannot_publish_changes() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("a", json!(1))]));
    let snapshot = store.persistence_snapshot()?;
    store.set_raw_attribute("a", json!(2))?;
    assert_code(
        store.publish_changes(&snapshot, raw(&[("a", json!(1))]), Map::new()),
        "model_attribute_snapshot_conflict",
    );
    assert!(store.get_changes()?.is_empty());
    let snapshot = store.persistence_snapshot()?;
    store.sync_original_attribute("a")?;
    assert_code(
        store.publish_changes(&snapshot, raw(&[("a", json!(2))]), Map::new()),
        "model_attribute_snapshot_conflict",
    );
    assert!(store.get_changes()?.is_empty());
    Ok(())
}

#[test]
fn cast_equivalence_obeys_null_numeric_float_and_hashed_rules() -> Result<()> {
    let metadata = metadata(&[
        ("int", AttributeCast::Integer),
        ("bool", AttributeCast::Boolean),
        ("float", AttributeCast::Float),
        ("hash", AttributeCast::Hashed),
    ]);
    for (key, old, new, expected) in [
        ("int", json!("00042"), json!(42), true),
        ("int", Value::Null, json!(0), false),
        ("bool", json!("0"), json!(false), true),
        ("float", json!(1.0), json!(1.0 + f64::EPSILON * 2.0), true),
        ("float", json!(1.0), json!(1.0 + f64::EPSILON * 4.0), false),
        ("float", Value::Null, json!(0.0), false),
        ("hash", json!("1"), json!(1), false),
        ("raw", json!("1"), json!(1), true),
        ("raw", json!("01"), json!(1), false),
        ("raw", json!("1.0"), json!(1), false),
        ("raw", json!(1), Value::Null, false),
    ] {
        let store = ModelAttributes::new(raw(&[(key, old)]));
        store.set_raw_attribute(key, new)?;
        assert_eq!(
            store.original_is_equivalent(&metadata, key)?,
            expected,
            "{key}"
        );
    }
    let empty = ModelAttributes::default();
    empty.set_raw_attribute("new", Value::Null)?;
    assert!(!empty.original_is_equivalent(&metadata, "new")?);
    let store = ModelAttributes::new(raw(&[("nullable", Value::Null)]));
    store.set_raw_attributes(Map::new())?;
    assert!(store.original_is_equivalent(&metadata, "nullable")?);
    assert!(store.get_dirty(&metadata)?.is_empty());
    let store = ModelAttributes::new(raw(&[("float", json!(1))]));
    store.set_raw_attribute("float", json!("Infinity"))?;
    assert_eq!(
        store.get_dirty(&metadata)?,
        raw(&[("float", json!("Infinity"))])
    );
    assert_eq!(store.get_raw_original("float")?, Some(json!(1)));
    store.sync_changes(&metadata)?;
    assert_eq!(store.get_changes()?, raw(&[("float", json!("Infinity"))]));
    let store = ModelAttributes::new(raw(&[("id", json!(1))]));
    store.set_raw_attribute("id", json!(2))?;
    let mut unsupported = metadata;
    unsupported.key_type = "unsupported".into();
    assert_code(
        store.get_dirty(&unsupported),
        "model_attribute_cast_unsupported",
    );
    Ok(())
}

#[test]
fn only_decoded_array_cast_normalizes_php_keys_and_empty_shape() -> Result<()> {
    let metadata = metadata(&[("array", AttributeCast::Array)]);
    for (old, new, expected) in [
        (json!("{}"), json!("[]"), true),
        (json!("{\"0\":1}"), json!("[1]"), true),
        (json!("{\"00\":1}"), json!("[1]"), false),
        (
            json!("{\"a\":1,\"b\":2}"),
            json!("{\"b\":2,\"a\":1}"),
            false,
        ),
        (json!("{\"nested\":{}}"), json!("{\"nested\":[]}"), true),
        (json!("[1]"), json!("[1.0]"), false),
    ] {
        let store = ModelAttributes::new(raw(&[("array", old)]));
        store.set_raw_attribute("array", new.clone())?;
        assert_eq!(store.original_is_equivalent(&metadata, "array")?, expected);
        assert_eq!(store.raw_attribute("array")?, Some(new));
    }
    let store = ModelAttributes::new(raw(&[("raw", json!({}))]));
    store.set_raw_attribute("raw", json!([]))?;
    assert!(store.is_dirty(&metadata, &["raw"])?);
    Ok(())
}

struct ForbiddenGetter;
impl AttributeGetter for ForbiddenGetter {
    fn get(&self, _: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
        Err(persistence_error("getter_must_not_run"))
    }
}
#[test]
fn comparison_bypasses_registered_getter() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("a", json!("1"))]));
    store.register_getter("a", Arc::new(ForbiddenGetter))?;
    store.set_raw_attribute("a", json!(1))?;
    assert!(store.is_clean(&metadata(&[]), &[])?);
    store.sync_changes(&metadata(&[]))?;
    Ok(())
}

struct WaitingSetter(Arc<tokio::sync::Notify>);
impl AttributeSetter for WaitingSetter {
    fn set(&self, _: Value, _: Map<String, Value>) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            self.0.notify_one();
            std::future::pending().await
        })
    }
}
#[tokio::test]
async fn active_setter_cannot_be_marked_clean_and_cancellation_releases_gate() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("a", json!(1))])));
    let entered = Arc::new(tokio::sync::Notify::new());
    store.register_setter("a", Arc::new(WaitingSetter(Arc::clone(&entered))))?;
    let running = Arc::clone(&store);
    let task =
        tokio::spawn(async move { running.set_attribute(&metadata(&[]), "a", json!(2)).await });
    entered.notified().await;
    assert_code(store.sync_original(), "model_attribute_update_pending");
    assert_code(
        store.sync_original_attribute("a"),
        "model_attribute_update_pending",
    );
    assert_code(
        store.sync_changes(&metadata(&[])),
        "model_attribute_update_pending",
    );
    assert_code(
        store.get_dirty(&metadata(&[])),
        "model_attribute_update_pending",
    );
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(store.is_clean(&metadata(&[]), &[])?);
    Ok(())
}

#[test]
fn raw_original_prefers_literal_keys_and_traverses_without_shape_conversion() -> Result<()> {
    let store = ModelAttributes::new(raw(&[
        ("a.b", json!("literal")),
        (
            "a",
            json!({"b": "nested", "*": {}, "list": [null, {"00": [], "0": {}}]}),
        ),
        ("empty", json!({})),
        ("nullable", Value::Null),
    ]));
    assert_eq!(store.get_raw_original("a.b")?, Some(json!("literal")));
    assert_eq!(store.get_raw_original("a.*")?, Some(json!({})));
    assert_eq!(store.get_raw_original("a.list.0")?, Some(Value::Null));
    assert_eq!(store.get_raw_original("a.list.1.00")?, Some(json!([])));
    assert_eq!(store.get_raw_original("a.list.1.0")?, Some(json!({})));
    for key in [
        "a.list.00",
        "a.list.+0",
        "a.list.-0",
        "a.list.-1",
        "a.list.9223372036854775808",
        "a.list.*",
    ] {
        assert_eq!(store.get_raw_original(key)?, None, "{key}");
    }
    assert_eq!(
        store.get_raw_original_or("nullable", json!("fallback"))?,
        Value::Null
    );
    assert_eq!(
        store.get_raw_original_or("a.list.0", json!("fallback"))?,
        Value::Null
    );
    assert_eq!(store.get_raw_original_or("missing", json!([]))?, json!([]));
    assert_eq!(store.get_raw_original_or("empty", json!([]))?, json!({}));
    assert_eq!(store.raw_attribute("a.*")?, None);
    assert_eq!(
        store.get_raw_original_attributes()?.get("empty"),
        Some(&json!({}))
    );
    Ok(())
}

#[test]
fn equivalent_requires_original_top_level_key_before_point_lookup() -> Result<()> {
    let metadata = metadata(&[]);
    let store = ModelAttributes::new(raw(&[("a", json!({"b": 1}))]));
    assert!(!store.original_is_equivalent(&metadata, "a.b")?);
    let store = ModelAttributes::new(raw(&[("a.b", json!(1)), ("a", json!({"b": 9}))]));
    store.set_raw_attributes(raw(&[("a", json!({"b": 1}))]))?;
    assert!(store.original_is_equivalent(&metadata, "a.b")?);
    store.set_raw_attribute("a.b", json!(2))?;
    assert!(!store.original_is_equivalent(&metadata, "a.b")?);
    assert_eq!(store.get_dirty(&metadata)?.get("a.b"), Some(&json!(2)));
    assert_eq!(store.get_raw_original("a.b")?, Some(json!(1)));
    Ok(())
}

#[test]
fn php64_raw_numbers_short_circuit_cast_with_php_type_identity() -> Result<()> {
    let metadata = metadata(&[("hash", AttributeCast::Hashed)]);
    let wide = 9_223_372_036_854_775_808_u64;
    let double = f64::from_bits(0x43e0_0000_0000_0000);
    for key in ["id", "hash", "raw"] {
        for (old, new) in [(json!(wide), json!(double)), (json!(wide), json!(wide + 1))] {
            let store = ModelAttributes::new(raw(&[(key, old)]));
            store.set_raw_attribute(key, new)?;
            assert!(store.original_is_equivalent(&metadata, key)?, "{key}");
        }
    }
    for (old, new) in [
        (json!([wide]), json!([double])),
        (json!({"n": wide}), json!({"n": wide + 1})),
    ] {
        let store = ModelAttributes::new(raw(&[("raw", old)]));
        store.set_raw_attribute("raw", new)?;
        assert!(store.original_is_equivalent(&metadata, "raw")?);
    }
    for (old, new) in [
        (json!(1), json!(1.0)),
        (json!(i64::MAX), json!(double)),
        (json!(double), json!(f64::from_bits(0x43e0_0000_0000_0001))),
        (json!([1]), json!([1.0])),
        (json!({"n": i64::MAX}), json!({"n": double})),
    ] {
        let store = ModelAttributes::new(raw(&[("hash", old)]));
        store.set_raw_attribute("hash", new)?;
        assert!(!store.original_is_equivalent(&metadata, "hash")?);
    }
    Ok(())
}

#[test]
fn float_comparison_keeps_nonfinite_intermediates_without_public_value_output() -> Result<()> {
    let metadata = metadata(&[("float", AttributeCast::Float)]);
    for (old, new, expected) in [
        (json!(1), json!("Infinity"), false),
        (json!(1), json!("-Infinity"), false),
        (json!(1), json!("NaN"), false),
        (json!("Infinity"), json!("1e9999"), false),
        (json!("-Infinity"), json!("-1e9999"), false),
        (json!("NaN"), json!("NaN"), true),
        (json!("Infinity"), json!("Infinity"), true),
        (json!(0), json!("infinity"), true),
        (json!(0), json!("nan"), true),
        (Value::Null, json!("Infinity"), false),
        (json!("Infinity"), Value::Null, false),
    ] {
        let store = ModelAttributes::new(raw(&[("float", old)]));
        store.set_raw_attribute("float", new)?;
        assert_eq!(store.original_is_equivalent(&metadata, "float")?, expected);
    }
    let store = ModelAttributes::new(raw(&[("float", json!(1))]));
    store.set_raw_attribute("float", json!([]))?;
    assert_code(store.get_dirty(&metadata), "model_attribute_cast_failed");
    assert!(AttributeCast::Float.read(Some(json!("Infinity"))).is_err());
    assert!(AttributeCast::Float.read(Some(json!("1e9999"))).is_err());
    Ok(())
}

#[test]
fn array_comparison_uses_validated_ir_including_nonfinite_json_numbers() -> Result<()> {
    let metadata = metadata(&[("array", AttributeCast::Array)]);
    for (old, new, expected) in [
        ("[1e999]", "[1e1000]", true),
        ("[1e999]", "[-1e1000]", false),
        ("[9223372036854775808]", "[9223372036854775809]", true),
        ("[9223372036854775808]", "[9.223372036854776e18]", true),
        ("[9223372036854775807]", "[9.223372036854776e18]", false),
        ("[1]", "[1.0]", false),
        ("broken", "null", true),
        ("{\"n\":1e999,\"n\":1}", "{\"n\":1}", true),
        ("{\"n\":\"\\ud800\",\"n\":1}", "null", true),
    ] {
        let store = ModelAttributes::new(raw(&[("array", json!(old))]));
        store.set_raw_attribute("array", json!(new))?;
        assert_eq!(
            store.original_is_equivalent(&metadata, "array")?,
            expected,
            "{old} => {new}"
        );
        assert_eq!(store.raw_attribute("array")?, Some(json!(new)));
        if !expected {
            assert_eq!(store.get_dirty(&metadata)?, raw(&[("array", json!(new))]));
        }
    }
    assert!(AttributeCast::Array.read(Some(json!("[1e999]"))).is_err());
    assert_eq!(
        AttributeCast::Array.read(Some(json!("broken")))?,
        Some(Value::Null)
    );
    Ok(())
}

#[test]
fn sync_operations_return_the_same_receiver_and_support_chaining() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("a", json!(1))]));
    store.set_raw_attribute("a", json!(2))?;
    let synced = store
        .sync_changes(&metadata(&[]))?
        .sync_original_attribute("a")?
        .sync_original_attributes(&["a"])?
        .sync_original()?;
    assert!(std::ptr::eq(
        std::ptr::from_ref(synced),
        std::ptr::from_ref(&store)
    ));
    assert_eq!(synced.get_raw_original("a")?, Some(json!(2)));
    assert_eq!(synced.get_changes()?, raw(&[("a", json!(2))]));
    assert_eq!(synced.get_previous()?, raw(&[("a", json!(1))]));
    Ok(())
}

#[test]
fn dirty_uses_normalized_metadata_key_cast_without_special_id_rules() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("id", json!("01"))]));
    store.set_raw_attribute("id", json!(1))?;
    let mut metadata = metadata(&[]);
    metadata.key_type = "\0\t INT\r\n\u{b}".into();
    assert!(store.is_clean(&metadata, &["id"])?);
    metadata.key_type = " HaShEd ".into();
    assert_code(
        store.is_dirty(&metadata, &["id"]),
        "model_attribute_cast_unsupported",
    );
    assert_code(
        store.get_attribute(&metadata, "id"),
        "model_attribute_cast_unsupported",
    );
    metadata.key_type = "hashed".into();
    assert!(store.is_dirty(&metadata, &["id"])?);
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(json!(1)));
    metadata.casts.insert("id".into(), AttributeCast::Integer);
    assert!(store.is_clean(&metadata, &["id"])?);
    Ok(())
}

#[test]
fn raw_hashed_declaration_checks_follow_read_and_dirty_short_circuits() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("id", json!("01"))]));
    let mut metadata = metadata(&[]);
    metadata.key_type = " HaShEd ".into();
    assert!(store.is_clean(&metadata, &[])?);
    assert_code(
        store.get_attribute(&metadata, "id"),
        "model_attribute_cast_unsupported",
    );
    store.set_raw_attribute("id", Value::Null)?;
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(Value::Null));
    assert!(store.is_dirty(&metadata, &[])?);
    store.set_raw_attribute("id", json!(1))?;
    assert_code(
        store.sync_changes(&metadata),
        "model_attribute_cast_unsupported",
    );
    assert!(store.get_changes()?.is_empty());
    assert!(store.get_previous()?.is_empty());
    metadata.key_type = "hashed".into();
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(json!(1)));
    assert!(store.is_dirty(&metadata, &[])?);
    store.sync_changes(&metadata)?;
    metadata.key_type = " HaShEd ".into();
    assert_code(
        store.sync_changes(&metadata),
        "model_attribute_cast_unsupported",
    );
    assert_eq!(store.get_changes()?, raw(&[("id", json!(1))]));
    assert_eq!(store.get_previous()?, raw(&[("id", json!("01"))]));
    metadata.casts.insert("id".into(), AttributeCast::Hashed);
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(json!(1)));
    assert!(store.is_dirty(&metadata, &[])?);
    Ok(())
}

#[tokio::test]
async fn raw_cast_write_checks_precede_null_json_and_hash_dispatch() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("id", json!("01"))]));
    let mut metadata = metadata(&[]);
    metadata.key_type = " INT ".into();
    store.set_raw_attribute("id", json!(1))?;
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(json!(1)));
    assert!(store.is_clean(&metadata, &[])?);
    for declaration in [" INT ", " HaShEd ", " JSON "] {
        metadata.key_type = declaration.into();
        for value in [json!(2), Value::Null] {
            assert_code(
                store.set_attribute(&metadata, "id", value.clone()).await,
                "model_attribute_cast_unsupported",
            );
            assert_code(
                store
                    .set_attribute_reversibly_if(&metadata, "id", value, |_| Ok(true))
                    .await,
                "model_attribute_cast_unsupported",
            );
            assert_eq!(store.raw_attribute("id")?, Some(json!(1)));
            let state = store.state.read().map_err(|_| state_error())?;
            assert_eq!(state.pending_count, 0);
            assert_eq!(state.active_setters, 0);
        }
    }
    metadata.casts.insert("id".into(), AttributeCast::Integer);
    store.set_attribute(&metadata, "id", json!("02")).await?;
    assert_eq!(store.raw_attribute("id")?, Some(json!("02")));
    metadata.casts.clear();
    metadata.incrementing = false;
    store
        .set_attribute(&metadata, "id", json!("untyped"))
        .await?;
    assert_eq!(
        store.get_attribute(&metadata, "id")?,
        Some(json!("untyped"))
    );
    metadata.incrementing = true;
    metadata.key_type = "hashed".into();
    store.set_attribute(&metadata, "id", Value::Null).await?;
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(Value::Null));
    Ok(())
}

struct IdentitySetter;
impl AttributeSetter for IdentitySetter {
    fn set(
        &self,
        value: Value,
        _: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move { Ok(raw(&[("id", value)])) })
    }
}
#[tokio::test]
async fn registered_setter_precedes_invalid_raw_cast_declaration() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("id", json!(1))]));
    let mut metadata = metadata(&[]);
    metadata.key_type = " INT ".into();
    store.register_setter("id", Arc::new(IdentitySetter))?;
    store
        .set_attribute(&metadata, "id", json!("custom"))
        .await?;
    assert_eq!(store.raw_attribute("id")?, Some(json!("custom")));
    let state = store.state.read().map_err(|_| state_error())?;
    assert_eq!(state.pending_count, 0);
    assert_eq!(state.active_setters, 0);
    Ok(())
}
