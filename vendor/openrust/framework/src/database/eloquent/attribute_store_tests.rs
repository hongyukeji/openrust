//! 属性读取/写入优先级、原始快照、原子修改与锁外访问器回归。

use super::*;
use serde_json::json;
use std::sync::Weak;

#[test]
fn appended_accessors_follow_visibility_order_and_raw_input() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let store = ModelAttributes::new(raw(&[("stored", json!(10))]));
    let mut metadata = metadata(&[("stored", AttributeCast::String)]);
    metadata
        .set_appends(vec!["computed".into(), "computed".into(), "hidden".into()])
        .merge_appends(vec!["stored".into()]);
    metadata.hidden.push("hidden".into());
    assert!(metadata.has_appended("computed"));
    let calls = Arc::new(AtomicUsize::new(0));
    store.register_attribute(
        "computed",
        ModelAttribute::new().get({
            let calls = calls.clone();
            move |value, snapshot| {
                calls.fetch_add(1, Ordering::Relaxed);
                assert!(value.is_none());
                Ok(snapshot["stored"].clone())
            }
        }),
    )?;
    store.register_attribute(
        "stored",
        ModelAttribute::new().get(|value, _| {
            assert_eq!(value.ok_or_else(failure)?.as_json()?, &json!(10));
            Ok(json!("accessor").into())
        }),
    )?;
    let values = store.attributes_to_array(&metadata)?;
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        ["stored", "computed"]
    );
    assert_eq!(values["computed"].as_json()?, &json!(10));
    assert_eq!(values["stored"].as_json()?, &json!("accessor"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(store.raw_attribute("computed")?.is_none());
    metadata.visible = vec!["stored".into()];
    assert!(
        !store
            .attributes_to_array(&metadata)?
            .contains_key("computed")
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    metadata.visible.clear();
    metadata.append("missing");
    assert_eq!(
        store
            .attributes_to_array(&metadata)
            .err()
            .map(|error| error.code().to_owned())
            .as_deref(),
        Some("model_appended_accessor_missing"),
    );
    metadata.without_appends();
    assert_eq!(metadata.get_appends(), Vec::<String>::new());
    assert_eq!(store.attributes_to_array(&metadata)?.len(), 1);
    metadata.key_name = "computed".into();
    metadata.key_type = " INT ".into();
    metadata.append("computed");
    let calls_before = calls.load(Ordering::Relaxed);
    assert!(matches!(
        store.attributes_to_array(&metadata),
        Err(error) if error.code() == "model_attribute_cast_unsupported"
    ));
    assert_eq!(calls.load(Ordering::Relaxed), calls_before);
    Ok(())
}

struct DateSnapshotAccessor;

impl AttributeGetter for DateSnapshotAccessor {
    fn get(
        &self,
        _raw: Option<&AttributeValue>,
        snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Ok(snapshot.get("hidden_date").cloned())
    }
}

impl AttributeSetter for DateSnapshotAccessor {
    fn set(
        &self,
        value: AttributeValue,
        snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            assert!(snapshot["hidden_date"].as_date().is_some());
            Ok(AttributeMap::from_iter([("label".into(), value)]))
        })
    }
}

#[tokio::test]
async fn legacy_accessors_share_typed_dates_without_projecting_the_snapshot() -> Result<()> {
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};

    let date = super::super::ModelDate::new(
        InputDate::new(CalendarInstant::new(20, 0)?, ResolvedTimezone::utc()),
        false,
    );
    let mut metadata = metadata(&[]);
    metadata.hidden.push("hidden_date".into());
    let store = ModelAttributes::new(raw(&[("label", json!("original"))])).with_date_context(
        ModelDateContext::default().with_serializer(|_| Ok("model override".into())),
    );
    store.set_raw_attribute_value("hidden_date", date.clone().into())?;
    store.register_getter("label", Arc::new(DateSnapshotAccessor))?;
    store.register_setter("label", Arc::new(DateSnapshotAccessor))?;
    store
        .set_attribute(&metadata, "label", json!("updated"))
        .await?;
    assert_eq!(store.raw_attribute("label")?, Some(json!("updated")));
    let projected = store.attributes_to_array(&metadata)?;
    assert!(!projected.contains_key("hidden_date"));
    assert!(
        projected["label"]
            .as_date()
            .ok_or_else(failure)?
            .same_object(&date)
    );
    assert_eq!(
        store.attributes_to_json(&metadata)?["label"],
        json!("1970-01-01T00:00:20.000000Z")
    );
    Ok(())
}

fn metadata(casts: &[(&str, AttributeCast)]) -> ModelMetadata {
    let mut metadata = ModelMetadata::new("records", "app::Record");
    metadata.casts = casts
        .iter()
        .map(|(name, cast)| ((*name).into(), cast.clone()))
        .collect();
    metadata
}
fn raw(values: &[(&str, Value)]) -> Map<String, Value> {
    values
        .iter()
        .map(|(name, value)| ((*name).into(), value.clone()))
        .collect()
}
fn failure() -> Error {
    Error::custom("attribute_fixture_failure", "属性测试拒绝")
}

#[test]
fn hydration_and_reads_leave_raw_unchanged_and_debug_redacts() -> Result<()> {
    let values = raw(&[
        ("id", json!("00042")),
        ("secret", json!("private")),
        ("list", json!("[1,2]")),
    ]);
    let store = ModelAttributes::new(values.clone());
    let metadata = metadata(&[
        ("secret", AttributeCast::Hashed),
        ("list", AttributeCast::Array),
    ]);
    assert_eq!(store.get_attribute(&metadata, "id")?, Some(json!(42)));
    assert_eq!(store.get_attribute(&metadata, "list")?, Some(json!([1, 2])));
    assert_eq!(
        store.get_attribute(&metadata, "secret")?,
        Some(json!("private"))
    );
    assert_eq!(store.get_attribute(&metadata, "missing")?, None);
    assert_eq!(store.raw_attributes()?, values);
    assert_eq!(format!("{store:?}"), "ModelAttributes([REDACTED])");
    store.set_raw_attribute("secret", json!("unvalidated-encoded"))?;
    assert_eq!(
        store.raw_attribute("secret")?,
        Some(json!("unvalidated-encoded"))
    );
    store.set_raw_attributes(Map::new())?;
    assert_eq!(store.raw_attributes()?, Map::new());
    Ok(())
}

struct ReentrantGetter(Weak<ModelAttributes>);
impl AttributeGetter for ReentrantGetter {
    fn get(
        &self,
        value: Option<&AttributeValue>,
        snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        let store = self.0.upgrade().ok_or_else(failure)?;
        assert_eq!(
            store.raw_attribute("id")?,
            snapshot
                .get("id")
                .map(AttributeValue::as_json)
                .transpose()?
                .cloned()
        );
        store.set_raw_attribute("getter_side_effect", json!(true))?;
        Ok(value.map(|_| json!("custom getter").into()))
    }
}
#[test]
fn custom_getter_precedes_even_unsupported_implicit_cast_and_can_reenter() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("id", json!("raw-id"))])));
    let mut metadata = metadata(&[]);
    metadata.key_type = "unsupported".into();
    store.register_getter("id", Arc::new(ReentrantGetter(Arc::downgrade(&store))))?;
    assert_eq!(
        store.get_attribute(&metadata, "id")?,
        Some(json!("custom getter"))
    );
    assert_eq!(store.raw_attribute("id")?, Some(json!("raw-id")));
    assert_eq!(
        store.raw_attribute("getter_side_effect")?,
        Some(json!(true))
    );
    Ok(())
}

struct ReentrantSetter(Weak<ModelAttributes>);
impl AttributeSetter for ReentrantSetter {
    fn set(
        &self,
        value: AttributeValue,
        snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            let store = self.0.upgrade().ok_or_else(failure)?;
            assert_eq!(
                store.raw_attribute("untouched")?,
                snapshot
                    .get("untouched")
                    .map(AttributeValue::as_json)
                    .transpose()?
                    .cloned()
            );
            store.set_raw_attribute("during_await", json!("preserved"))?;
            tokio::task::yield_now().await;
            Ok(AttributeMap::from_iter([
                ("secret".into(), value),
                ("derived".into(), json!(true).into()),
            ]))
        })
    }
}
#[tokio::test]
async fn custom_setter_precedes_hashed_cast_and_merges_multi_field_patch_after_await() -> Result<()>
{
    let store = Arc::new(ModelAttributes::new(raw(&[("untouched", json!(7))])));
    store.register_setter("secret", Arc::new(ReentrantSetter(Arc::downgrade(&store))))?;
    store
        .set_attribute(
            &metadata(&[("secret", AttributeCast::Hashed)]),
            "secret",
            json!("custom"),
        )
        .await?;
    assert_eq!(
        store.raw_attributes()?,
        raw(&[
            ("untouched", json!(7)),
            ("during_await", json!("preserved")),
            ("secret", json!("custom")),
            ("derived", json!(true))
        ])
    );
    Ok(())
}

#[tokio::test]
async fn guardable_setter_short_circuits_schema_and_cast_validation() -> Result<()> {
    let attributes = ModelAttributes::default();
    let databases = Databases::new(crate::Db::default(), BTreeMap::new());
    let mut model = ModelMetadata::new("records", "example::Record");
    model.guard(vec!["owner_id".into()]);
    model.connection = Some("unconfigured".into());
    model.key_name = "virtual_key".into();
    model.key_type = "UnknownCaster".into();

    assert!(matches!(
        attributes.is_guarded(&model, &databases, "virtual_key").await,
        Err(error) if error.code() == "model_attribute_cast_unsupported"
    ));
    attributes.register_setter("virtual_key", Arc::new(FailingSetter))?;
    assert!(
        !attributes
            .is_guarded(&model, &databases, "virtual_key")
            .await?
    );
    assert!(
        attributes
            .is_fillable(&model, &databases, "virtual_key")
            .await?
    );
    assert!(attributes.raw_attribute("virtual_key")?.is_none());

    model
        .casts
        .insert("primitive".into(), AttributeCast::Hashed);
    assert!(matches!(
        attributes.is_guarded(&model, &databases, "primitive").await,
        Err(error) if error.code() == "database_instance_not_found"
    ));
    Ok(())
}

struct FailingSetter;
impl AttributeSetter for FailingSetter {
    fn set(
        &self,
        _value: AttributeValue,
        mut snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            snapshot.insert("partial".into(), json!(true).into());
            tokio::task::yield_now().await;
            Err(failure())
        })
    }
}
#[tokio::test]
async fn setter_failure_and_missing_hashing_context_leave_raw_unchanged() -> Result<()> {
    let values = raw(&[("secret", json!("stored"))]);
    let store = ModelAttributes::new(values.clone());
    let metadata = metadata(&[("secret", AttributeCast::Hashed)]);
    assert!(
        matches!(store.set_attribute(&metadata, "secret", json!("new")).await,
        Err(error) if error.code() == "model_attribute_hashing_required" && error.status().as_u16() == 500)
    );
    assert_eq!(store.raw_attributes()?, values);
    store
        .set_attribute(&metadata, "secret", Value::Null)
        .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(Value::Null));
    store.set_raw_attributes(values.clone())?;
    store.register_setter("secret", Arc::new(FailingSetter))?;
    assert!(
        matches!(store.set_attribute(&metadata, "secret", json!("new")).await,
        Err(error) if error.code() == "attribute_fixture_failure")
    );
    assert_eq!(store.raw_attributes()?, values);
    Ok(())
}

#[tokio::test]
async fn ordinary_assignment_keeps_primitive_raw_and_encodes_array_only() -> Result<()> {
    let store = ModelAttributes::default();
    let metadata = metadata(&[
        ("age", AttributeCast::Integer),
        ("list", AttributeCast::Array),
    ]);
    store
        .set_attribute(&metadata, "age", json!("0042tail"))
        .await?;
    assert_eq!(store.raw_attribute("age")?, Some(json!("0042tail")));
    assert_eq!(store.get_attribute(&metadata, "age")?, Some(json!(42)));
    store
        .set_attribute(&metadata, "list", json!({"b":[1,2]}))
        .await?;
    assert_eq!(store.raw_attribute("list")?, Some(json!("{\"b\":[1,2]}")));
    assert_eq!(
        store.get_attribute(&metadata, "list")?,
        Some(json!({"b":[1,2]}))
    );
    store.set_attribute(&metadata, "list", Value::Null).await?;
    assert_eq!(store.raw_attribute("list")?, Some(Value::Null));
    for value in ["", "broken"] {
        store.set_raw_attribute("list", json!(value))?;
        assert_eq!(store.get_attribute(&metadata, "list")?, Some(Value::Null));
        assert_eq!(store.raw_attribute("list")?, Some(json!(value)));
    }
    Ok(())
}

#[test]
fn atomic_raw_update_returns_value_and_discards_failed_candidate() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("counter", json!(0))]));
    let owned = "once".to_owned();
    let result = store.update_raw_attributes(move |values| {
        values.insert("counter".into(), json!(1));
        Ok(owned)
    })?;
    assert_eq!(result, "once");
    let result: Result<()> = store.update_raw_attributes(|values| {
        values.insert("counter".into(), json!(999));
        Err(failure())
    });
    assert!(matches!(result, Err(error) if error.code() == "attribute_fixture_failure"));
    assert_eq!(store.raw_attribute("counter")?, Some(json!(1)));
    Ok(())
}

#[test]
fn concurrent_raw_updates_serialize_their_compare_and_write() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("counter", json!(0))])));
    std::thread::scope(|scope| -> Result<()> {
        let mut workers = Vec::new();
        for _ in 0..4 {
            let store = store.clone();
            workers.push(scope.spawn(move || -> Result<()> {
                for _ in 0..25 {
                    store.update_raw_attributes(|values| {
                        let current = values
                            .get("counter")
                            .and_then(Value::as_i64)
                            .ok_or_else(failure)?;
                        values.insert("counter".into(), json!(current + 1));
                        Ok(())
                    })?;
                }
                Ok(())
            }));
        }
        for worker in workers {
            worker.join().map_err(|_| failure())??;
        }
        Ok(())
    })?;
    assert_eq!(store.raw_attribute("counter")?, Some(json!(100)));
    Ok(())
}

#[cfg(feature = "hashing")]
#[tokio::test]
async fn explicit_shared_hash_manager_casts_secret_and_null_without_auth_input_policy() -> Result<()>
{
    use crate::hashing::{BcryptConfig, HashManager, HashOptions, HashingConfig};
    let hashing = HashManager::new(HashingConfig {
        bcrypt: BcryptConfig {
            rounds: 4,
            verify: true,
            limit: None,
        },
        ..Default::default()
    })?;
    let store = ModelAttributes::default();
    let metadata = metadata(&[("secret", AttributeCast::Hashed)]);
    // 通用模型cast不套增强Auth的72字节策略。
    let plain = "x".repeat(73);
    store
        .set_attribute_with_hashing(&metadata, "secret", json!(plain), &hashing)
        .await?;
    let encoded = store
        .raw_attribute("secret")?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(failure)?;
    assert!(
        hashing
            .check(&plain, Some(&encoded), &HashOptions::new())
            .await?
    );
    store
        .set_attribute_with_hashing(&metadata, "secret", json!(encoded), &hashing)
        .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(json!(encoded)));
    assert!(
        store
            .set_attribute_with_hashing(&metadata, "secret", json!(["not scalar"]), &hashing)
            .await
            .is_err()
    );
    assert_eq!(store.raw_attribute("secret")?, Some(json!(encoded)));
    store
        .set_attribute_with_hashing(&metadata, "secret", Value::Null, &hashing)
        .await?;
    assert_eq!(store.raw_attribute("secret")?, Some(Value::Null));
    Ok(())
}

struct DropAccessor {
    store: Weak<ModelAttributes>,
    released: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for DropAccessor {
    fn drop(&mut self) {
        if let Some(store) = self.store.upgrade() {
            self.released.store(
                store.raw_attributes().is_ok(),
                std::sync::atomic::Ordering::SeqCst,
            );
        }
    }
}
impl AttributeGetter for DropAccessor {
    fn get(
        &self,
        raw: Option<&AttributeValue>,
        _snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Ok(raw.cloned())
    }
}
impl AttributeSetter for DropAccessor {
    fn set(
        &self,
        value: AttributeValue,
        _snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move { Ok(AttributeMap::from_iter([("field".into(), value)])) })
    }
}
#[test]
fn replaced_accessors_drop_outside_storage_lock() -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let store = Arc::new(ModelAttributes::default());
    let getter_released = Arc::new(AtomicBool::new(false));
    let setter_released = Arc::new(AtomicBool::new(false));
    store.register_getter(
        "field",
        Arc::new(DropAccessor {
            store: Arc::downgrade(&store),
            released: getter_released.clone(),
        }),
    )?;
    store.register_setter(
        "field",
        Arc::new(DropAccessor {
            store: Arc::downgrade(&store),
            released: setter_released.clone(),
        }),
    )?;
    store.register_getter("field", Arc::new(ReentrantGetter(Arc::downgrade(&store))))?;
    store.register_setter("field", Arc::new(FailingSetter))?;
    assert!(getter_released.load(Ordering::SeqCst));
    assert!(setter_released.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn current_attribute_selection_keeps_missing_keys_and_freezes_except_keys() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(Map::from_iter([
        ("id".into(), json!("42")),
        ("secret".into(), json!("visible to explicit reads")),
        ("nullable".into(), Value::Null),
    ])));
    let mut metadata = ModelMetadata::new("records", "example::Record");
    metadata.hidden.push("secret".into());
    store.register_getter("id", Arc::new(ReentrantGetter(Arc::downgrade(&store))))?;
    let excluded = store.except(&metadata, &["nullable"])?;
    assert_eq!(excluded.get("id"), Some(&json!("custom getter")));
    assert_eq!(
        excluded.get("secret"),
        Some(&json!("visible to explicit reads"))
    );
    assert!(!excluded.contains_key("getter_side_effect"));
    assert!(!excluded.contains_key("nullable"));

    let selected = store.only(
        &metadata,
        &["missing", "nullable", "getter_side_effect", "id"],
    )?;
    assert_eq!(selected.get("missing"), Some(&Value::Null));
    assert_eq!(selected.get("nullable"), Some(&Value::Null));
    assert_eq!(selected.get("getter_side_effect"), Some(&json!(true)));
    assert_eq!(selected.get("id"), Some(&json!("custom getter")));

    store.register_getter("id", Arc::new(FailingGetter))?;
    assert!(store.only(&metadata, &["id"]).is_err());
    assert!(store.except(&metadata, &[]).is_err());
    assert!(store.except(&metadata, &["id"]).is_ok());
    Ok(())
}

#[test]
fn except_compares_integer_and_string_attribute_keys_using_php_membership() -> Result<()> {
    let store = ModelAttributes::new(Map::from_iter([
        (String::new(), json!(0)),
        ("0".into(), json!(0)),
        ("1".into(), json!(1)),
        ("01".into(), json!(2)),
        ("1x".into(), json!(3)),
        ("9223372036854775807".into(), json!(4)),
        ("9223372036854775809".into(), json!(5)),
    ]));
    let metadata = ModelMetadata::new("records", "example::Record");
    store.register_getter("0", Arc::new(FailingGetter))?;

    let selected = store.except(&metadata, &["1e0", "9223372036854775808"])?;
    assert_eq!(
        selected,
        Map::from_iter([
            (String::new(), Value::Null),
            ("0".into(), Value::Null),
            ("1x".into(), json!(3)),
            ("9223372036854775809".into(), json!(5)),
        ])
    );

    let selected = store.only(&metadata, &["", "0", "1", "01", "1e0"])?;
    assert_eq!(selected.get(""), Some(&Value::Null));
    assert_eq!(selected.get("0"), Some(&Value::Null));
    assert_eq!(selected.get("1"), Some(&json!(1)));
    assert_eq!(selected.get("01"), Some(&json!(2)));
    assert_eq!(selected.get("1e0"), Some(&Value::Null));
    Ok(())
}

#[test]
fn save_query_key_uses_raw_literal_original_before_current_accessor() -> Result<()> {
    let mut model = ModelMetadata::new("records", "example::Record");
    let attributes = ModelAttributes::new(Map::from_iter([("id".into(), json!("0042"))]));
    attributes.set_raw_attribute("id", json!("0059"))?;
    attributes.register_getter("id", Arc::new(FailingGetter))?;
    assert_eq!(attributes.key_for_save_query(&model)?, json!("0042"));
    assert!(attributes.get_attribute(&model, "id").is_err());

    let null_original = ModelAttributes::new(Map::from_iter([("id".into(), Value::Null)]));
    null_original.set_raw_attribute("id", json!("0059"))?;
    assert_eq!(null_original.key_for_save_query(&model)?, json!(59));
    null_original.register_getter("id", Arc::new(FailingGetter))?;
    assert!(null_original.key_for_save_query(&model).is_err());

    let dotted = ModelAttributes::new(Map::from_iter([("scope".into(), json!({"id": 42}))]));
    model.key_name = "scope.id".into();
    dotted.set_raw_attribute("scope.id", json!("0059"))?;
    assert_eq!(dotted.get_raw_original("scope.id")?, Some(json!(42)));
    assert_eq!(dotted.key_for_save_query(&model)?, json!(59));

    let zero = ModelAttributes::new(Map::from_iter([("id".into(), json!(0))]));
    model.key_name = "id".into();
    zero.set_raw_attribute("id", json!(59))?;
    assert_eq!(zero.key_for_save_query(&model)?, json!(0));
    model.key_name = "0".into();
    zero.set_raw_attribute("0", json!(59))?;
    assert_eq!(zero.key_for_save_query(&model)?, Value::Null);
    Ok(())
}

struct FailingGetter;
impl AttributeGetter for FailingGetter {
    fn get(
        &self,
        _value: Option<&AttributeValue>,
        _snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Err(failure())
    }
}
#[test]
fn getter_failure_does_not_mutate_raw_or_fall_through_to_cast() -> Result<()> {
    let values = raw(&[("field", json!("42"))]);
    let store = ModelAttributes::new(values.clone());
    store.register_getter("field", Arc::new(FailingGetter))?;
    assert!(
        matches!(store.get_attribute(&metadata(&[("field", AttributeCast::Integer)]), "field"),
        Err(error) if error.code() == "attribute_fixture_failure")
    );
    assert_eq!(store.raw_attributes()?, values);
    Ok(())
}

#[test]
fn array_decode_uses_php64_numeric_categories_and_never_hides_infinity() -> Result<()> {
    let metadata = metadata(&[("list", AttributeCast::Array)]);
    let store = ModelAttributes::default();
    store.set_raw_attribute(
        "list",
        json!("[9223372036854775807,9223372036854775808,18446744073709551615,1.0,1e-5]"),
    )?;
    let decoded = store
        .get_attribute(&metadata, "list")?
        .ok_or_else(failure)?;
    let values = decoded.as_array().ok_or_else(failure)?;
    assert_eq!(values[0].as_i64(), Some(i64::MAX));
    for value in &values[1..] {
        assert!(value.as_number().is_some_and(serde_json::Number::is_f64));
    }
    assert_eq!(values[1].as_f64(), Some(9_223_372_036_854_775_808.0));
    assert_eq!(values[2].as_f64(), Some(18_446_744_073_709_551_616.0));
    for text in ["1e9999", "{\"nested\":[-1e9999]}"] {
        store.set_raw_attribute("list", json!(text))?;
        assert!(matches!(store.get_attribute(&metadata, "list"), Err(error)
            if error.code() == "model_attribute_cast_failed" && error.status().as_u16() == 500));
        assert_eq!(store.raw_attribute("list")?, Some(json!(text)));
    }
    for text in ["1e9999 invalid", "[1,]", ""] {
        store.set_raw_attribute("list", json!(text))?;
        assert_eq!(store.get_attribute(&metadata, "list")?, Some(Value::Null));
    }
    Ok(())
}

#[tokio::test]
async fn array_encode_matches_default_unicode_slashes_and_float_shapes() -> Result<()> {
    let metadata = metadata(&[("list", AttributeCast::Array)]);
    let store = ModelAttributes::default();
    for (value, expected) in [
        (
            json!({"中文":"/😀\n"}),
            r#"{"\u4e2d\u6587":"\/\ud83d\ude00\n"}"#,
        ),
        (json!(1.0), "1"),
        (json!(-0.0), "-0"),
        (json!(0.0001), "0.0001"),
        (json!(0.00001), "1.0e-5"),
        (json!(1e16), "10000000000000000"),
        (json!(1e17), "1.0e+17"),
        (json!(1.25e20), "1.25e+20"),
    ] {
        store.set_attribute(&metadata, "list", value).await?;
        assert_eq!(store.raw_attribute("list")?, Some(json!(expected)));
    }
    Ok(())
}

#[test]
fn model_state_faults_are_server_errors() {
    assert_eq!(super::state_error().status().as_u16(), 500);
    assert_eq!(super::hashing_required().status().as_u16(), 500);
}

struct NullDefaultGetter;
impl AttributeGetter for NullDefaultGetter {
    fn get(
        &self,
        _raw: Option<&AttributeValue>,
        _snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Ok(Some(Value::Null.into()))
    }
}

#[test]
fn default_getter_does_not_fall_through_null_or_run_under_store_lock() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("id", json!("00042"))])));
    let mut metadata = metadata(&[]);
    metadata.key_type = "unsupported".into();
    assert_eq!(
        store.get_attribute_with_default_getter(&metadata, "id", &NullDefaultGetter)?,
        Some(Value::Null)
    );
    let reentrant = ReentrantGetter(Arc::downgrade(&store));
    assert_eq!(
        store.get_attribute_with_default_getter(&metadata, "id", &reentrant)?,
        Some(json!("custom getter"))
    );
    store.register_getter("id", Arc::new(NullDefaultGetter))?;
    assert_eq!(
        store.get_attribute_with_default_getter(&metadata, "id", &reentrant)?,
        Some(Value::Null)
    );
    Ok(())
}

#[test]
fn string_attribute_uses_selected_cast_and_preserves_missing_null() -> Result<()> {
    let metadata = metadata(&[("count", AttributeCast::Integer)]);
    let store = ModelAttributes::new(raw(&[
        ("count", json!("0012")),
        ("enabled", json!(true)),
        ("empty", Value::Null),
        ("list", json!([1, 2])),
    ]));
    assert_eq!(
        store.get_string_attribute(&metadata, "count")?,
        Some("12".into())
    );
    assert_eq!(
        store.get_string_attribute(&metadata, "enabled")?,
        Some("1".into())
    );
    assert_eq!(store.get_string_attribute(&metadata, "empty")?, None);
    assert_eq!(store.get_string_attribute(&metadata, "missing")?, None);
    assert_eq!(
        store
            .get_string_attribute(&metadata, "list")
            .err()
            .map(|error| error.status().as_u16()),
        Some(500)
    );
    assert_eq!(store.raw_attribute("count")?, Some(json!("0012")));
    Ok(())
}

struct PatchSetter;
impl AttributeSetter for PatchSetter {
    fn set(
        &self,
        value: AttributeValue,
        _snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            Ok(AttributeMap::from_iter([
                ("token".into(), value),
                ("missing".into(), Value::Null.into()),
                ("null".into(), json!("new").into()),
            ]))
        })
    }
}

#[tokio::test]
async fn reversible_patch_drop_preserves_missing_null_and_unrelated_successors() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("old")), ("null", Value::Null)]));
    let metadata = metadata(&[]);
    store.register_setter("token", Arc::new(PatchSetter))?;
    let receipt = store
        .set_attribute_reversibly_if(&metadata, "token", json!("new"), |current| {
            assert_eq!(current, Some(json!("old")));
            Ok(true)
        })
        .await?
        .ok_or_else(failure)?;
    assert_eq!(store.raw_attribute("missing")?, Some(Value::Null));
    store.set_raw_attribute("other", json!(7))?;
    drop(receipt);
    assert_eq!(store.raw_attribute("token")?, Some(json!("old")));
    assert_eq!(store.raw_attribute("missing")?, None);
    assert_eq!(store.raw_attribute("null")?, Some(Value::Null));
    assert_eq!(store.raw_attribute("other")?, Some(json!(7)));
    Ok(())
}

#[tokio::test]
async fn reversible_commit_aba_and_pending_cas_have_precise_ownership() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("old"))]));
    let metadata = metadata(&[]);
    let mut receipt = store
        .set_attribute_reversibly_if(&metadata, "token", json!("new"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    let mut successor = store
        .set_attribute_reversibly_if(&metadata, "token", json!("third"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    assert!(successor.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("new")));
    let mut called = false;
    assert!(
        matches!(store.update_raw_attributes(|_| { called = true; Ok(false) }), Err(error) if error.code() == "model_attribute_mutation_conflict")
    );
    assert!(!called);
    store.set_raw_attribute("token", json!("intermediate"))?;
    store.set_raw_attribute("token", json!("new"))?;
    assert!(!receipt.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("new")));
    let receipt = store
        .set_attribute_reversibly_if(&metadata, "token", json!("committed"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    receipt.commit()?;
    assert_eq!(store.raw_attribute("token")?, Some(json!("committed")));
    Ok(())
}

struct WaitingSetter(Arc<tokio::sync::Notify>);
impl AttributeSetter for WaitingSetter {
    fn set(
        &self,
        value: AttributeValue,
        _snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            self.0.notify_one();
            std::future::pending::<()>().await;
            Ok(AttributeMap::from_iter([("token".into(), value)]))
        })
    }
}

#[tokio::test]
async fn cancelled_prepare_releases_slot_without_applying_and_false_skips_setter() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("token", json!("old"))])));
    let metadata = metadata(&[]);
    let entered = Arc::new(tokio::sync::Notify::new());
    store.register_setter("token", Arc::new(WaitingSetter(Arc::clone(&entered))))?;
    assert!(
        store
            .set_attribute_reversibly_if(&metadata, "token", json!("ignored"), |_| Ok(false))
            .await?
            .is_none()
    );
    let running = Arc::clone(&store);
    let task = tokio::spawn(async move {
        running
            .set_attribute_reversibly_if(&metadata, "token", json!("new"), |_| Ok(true))
            .await
    });
    entered.notified().await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert_eq!(store.raw_attribute("token")?, Some(json!("old")));
    assert!(
        store
            .set_attribute_reversibly_if(&self::metadata(&[]), "token", json!("ignored"), |_| Ok(
                false
            ))
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn reversible_reentrant_setter_conflicts_without_overwriting_its_successor() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[
        ("untouched", json!(7)),
        ("secret", json!("old")),
    ])));
    store.register_setter("secret", Arc::new(ReentrantSetter(Arc::downgrade(&store))))?;
    assert!(
        matches!(store.set_attribute_reversibly_if(&metadata(&[]), "secret", json!("new"), |_| Ok(true)).await, Err(error) if error.code() == "model_attribute_mutation_conflict")
    );
    assert_eq!(store.raw_attribute("secret")?, Some(json!("old")));
    assert_eq!(store.raw_attribute("derived")?, None);
    assert_eq!(
        store.raw_attribute("during_await")?,
        Some(json!("preserved"))
    );
    Ok(())
}

#[allow(
    clippy::panic,
    reason = "验证不可信回调异常毒化原始锁后，收据仍安全恢复"
)]
#[tokio::test]
async fn poisoned_state_drop_restores_only_owned_raw_fields_and_keeps_public_fault() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("old"))]));
    let receipt = store
        .set_attribute_reversibly_if(&metadata(&[]), "token", json!("new"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = store
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        panic!("fixture raw-state poison");
    }));
    assert!(poison.is_err());
    drop(receipt);
    assert!(matches!(store.raw_attribute("token"), Err(error) if error.status().as_u16() == 500));
    let state = store
        .state
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        state
            .raw
            .get("token")
            .map(AttributeValue::as_json)
            .transpose()?,
        Some(&json!("old"))
    );
    assert_eq!(state.pending_count, 0);
    Ok(())
}

#[tokio::test]
async fn reversible_predicate_runs_outside_lock_and_registration_invalidates_prepare() -> Result<()>
{
    let store = ModelAttributes::new(raw(&[("token", json!("old"))]));
    let result = store
        .set_attribute_reversibly_if(&metadata(&[]), "token", json!("new"), |current| {
            assert_eq!(store.raw_attribute("token")?, current);
            store.register_setter("token", Arc::new(PatchSetter))?;
            Ok(true)
        })
        .await;
    assert!(matches!(result, Err(error) if error.code() == "model_attribute_mutation_conflict"));
    assert_eq!(store.raw_attribute("token")?, Some(json!("old")));
    assert!(
        store
            .set_attribute_reversibly_if(&metadata(&[]), "token", json!("unused"), |_| Ok(false))
            .await?
            .is_none()
    );
    Ok(())
}

#[test]
fn exhausted_revision_rejects_raw_write_without_partial_change() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("old"))]));
    store.state.write().map_err(|_| failure())?.revision = u64::MAX;
    assert!(
        matches!(store.set_raw_attribute("token", json!("new")), Err(error) if error.code() == "model_attribute_revision_exhausted")
    );
    assert_eq!(store.raw_attribute("token")?, Some(json!("old")));
    Ok(())
}

#[tokio::test]
async fn superseded_field_does_not_prevent_restoring_other_patch_fields() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("old")), ("null", Value::Null)]));
    store.register_setter("token", Arc::new(PatchSetter))?;
    let mut receipt = store
        .set_attribute_reversibly_if(&metadata(&[]), "token", json!("candidate"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    store.set_raw_attribute("token", json!("successor"))?;
    assert!(!receipt.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("successor")));
    assert_eq!(store.raw_attribute("missing")?, None);
    assert_eq!(store.raw_attribute("null")?, Some(Value::Null));
    Ok(())
}

struct ForbiddenRawAccessor;
impl AttributeGetter for ForbiddenRawAccessor {
    #[allow(
        clippy::panic,
        reason = "以不可信getter的异常对抗用例验证raw模式不会调用访问器"
    )]
    fn get(&self, _: Option<&AttributeValue>, _: &AttributeMap) -> Result<Option<AttributeValue>> {
        panic!("raw mode must not invoke getter");
    }
}
impl AttributeSetter for ForbiddenRawAccessor {
    #[allow(
        clippy::panic,
        reason = "以不可信setter的异常对抗用例验证raw模式不会调用访问器"
    )]
    fn set(&self, _: AttributeValue, _: AttributeMap) -> BoxFuture<'_, Result<AttributeMap>> {
        panic!("raw mode must not invoke setter");
    }
}

#[test]
fn raw_reversible_mode_bypasses_accessors_and_preserves_absence_and_null() -> Result<()> {
    let store = ModelAttributes::default();
    store.register_getter("token", Arc::new(ForbiddenRawAccessor))?;
    store.register_setter("token", Arc::new(ForbiddenRawAccessor))?;
    let mut receipt = store
        .set_raw_attribute_reversibly_if("token", Value::Null, |current| {
            assert_eq!(current, None);
            assert_eq!(store.raw_attribute("token")?, None);
            Ok(true)
        })?
        .ok_or_else(failure)?;
    assert_eq!(store.raw_attribute("token")?, Some(Value::Null));
    assert!(
        store
            .set_raw_attribute_reversibly_if("token", json!("ignored"), |_| Ok(false))?
            .is_none()
    );
    assert!(receipt.rollback()?);
    assert_eq!(store.raw_attribute("token")?, None);
    store.set_raw_attribute("token", Value::Null)?;
    assert!(
        store
            .set_raw_attribute_reversibly_if("token", json!("ignored"), |current| {
                assert_eq!(current, Some(Value::Null));
                Ok(false)
            })?
            .is_none()
    );
    assert_eq!(store.raw_attribute("token")?, Some(Value::Null));
    let receipt = store
        .set_raw_attribute_reversibly_if("token", json!("candidate"), |_| Ok(true))?
        .ok_or_else(failure)?;
    drop(receipt);
    assert_eq!(store.raw_attribute("token")?, Some(Value::Null));
    Ok(())
}

fn raw_receipt(store: &ModelAttributes, value: &str) -> Result<super::AttributeMutation> {
    store
        .set_raw_attribute_reversibly_if("token", json!(value), |_| Ok(true))?
        .ok_or_else(failure)
}

#[test]
fn cancelling_ancestor_then_descendant_restores_initial_raw_value() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut ancestor = raw_receipt(&store, "A")?;
    let mut descendant = raw_receipt(&store, "B")?;
    assert!(!ancestor.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("B")));
    assert!(descendant.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
    Ok(())
}

#[test]
fn cancelling_descendant_restores_pending_ancestor_ownership() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut ancestor = raw_receipt(&store, "A")?;
    let mut descendant = raw_receipt(&store, "B")?;
    assert!(descendant.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("A")));
    assert!(ancestor.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
    Ok(())
}

#[test]
fn committed_ancestor_is_stable_boundary_for_descendant_cancellation() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let ancestor = raw_receipt(&store, "A")?;
    let mut descendant = raw_receipt(&store, "B")?;
    ancestor.commit()?;
    assert_eq!(store.raw_attribute("token")?, Some(json!("B")));
    assert!(descendant.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("A")));
    assert_eq!(store.state.read().map_err(|_| failure())?.pending_count, 0);
    Ok(())
}

#[test]
fn committed_descendant_survives_ancestor_cancellation() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut ancestor = raw_receipt(&store, "A")?;
    let descendant = raw_receipt(&store, "B")?;
    descendant.commit()?;
    assert!(!ancestor.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("B")));
    Ok(())
}

#[tokio::test]
async fn multi_field_descendant_drop_skips_cancelled_ancestor_and_preserves_raw_aba() -> Result<()>
{
    let store = ModelAttributes::new(raw(&[("token", json!("initial")), ("null", Value::Null)]));
    store.register_setter("token", Arc::new(PatchSetter))?;
    let ancestor = store
        .set_attribute_reversibly_if(&metadata(&[]), "token", json!("A"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    let descendant = store
        .set_attribute_reversibly_if(&metadata(&[]), "token", json!("B"), |_| Ok(true))
        .await?
        .ok_or_else(failure)?;
    store.set_raw_attribute("token", json!("ABA"))?;
    store.set_raw_attribute("token", json!("B"))?;
    drop(ancestor);
    drop(descendant);
    assert_eq!(store.raw_attribute("token")?, Some(json!("B")));
    assert_eq!(store.raw_attribute("missing")?, None);
    assert_eq!(store.raw_attribute("null")?, Some(Value::Null));
    Ok(())
}

#[test]
fn ordinary_writes_reserve_terminal_revisions_for_all_outstanding_receipts() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut ancestor = raw_receipt(&store, "A")?;
    let mut descendant = raw_receipt(&store, "B")?;
    store.state.write().map_err(|_| failure())?.revision = u64::MAX - 2;
    assert!(
        matches!(store.set_raw_attribute("other", json!(true)), Err(error) if error.code() == "model_attribute_revision_exhausted")
    );
    assert!(!ancestor.rollback()?);
    assert!(descendant.rollback()?);
    assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
    assert_eq!(store.raw_attribute("other")?, None);
    assert_eq!(store.state.read().map_err(|_| failure())?.pending_count, 0);
    Ok(())
}

struct ConcurrentPrepareSetter {
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}
impl AttributeSetter for ConcurrentPrepareSetter {
    fn set(&self, value: AttributeValue, _: AttributeMap) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            self.entered.add_permits(1);
            self.release
                .acquire()
                .await
                .map_err(|_| failure())?
                .forget();
            Ok(AttributeMap::from_iter([("token".into(), value)]))
        })
    }
}

#[tokio::test]
async fn concurrent_preparations_are_allowed_but_only_fresh_snapshot_installs() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("token", json!("initial"))])));
    let entered = Arc::new(tokio::sync::Semaphore::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    store.register_setter(
        "token",
        Arc::new(ConcurrentPrepareSetter {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
    )?;
    let left_store = Arc::clone(&store);
    let left = tokio::spawn(async move {
        left_store
            .set_attribute_reversibly_if(&metadata(&[]), "token", json!("left"), |_| Ok(true))
            .await
    });
    let right_store = Arc::clone(&store);
    let right = tokio::spawn(async move {
        right_store
            .set_attribute_reversibly_if(&metadata(&[]), "token", json!("right"), |_| Ok(true))
            .await
    });
    entered
        .acquire_many(2)
        .await
        .map_err(|_| failure())?
        .forget();
    assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
    release.add_permits(2);
    let left = left.await.map_err(|_| failure())?;
    let right = right.await.map_err(|_| failure())?;
    let ((Ok(Some(receipt)), Err(error)) | (Err(error), Ok(Some(receipt)))) = (left, right) else {
        return Err(failure());
    };
    assert_eq!(error.code(), "model_attribute_mutation_conflict");
    drop(receipt);
    assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
    assert_eq!(store.state.read().map_err(|_| failure())?.pending_count, 0);
    Ok(())
}

#[tokio::test]
async fn ordinary_setter_rejects_same_field_committed_successor_and_delete_reinsert_aba()
-> Result<()> {
    for delete_reinsert in [false, true] {
        let store = Arc::new(ModelAttributes::new(raw(&[("token", json!("initial"))])));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        store.register_setter(
            "token",
            Arc::new(ConcurrentPrepareSetter {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            }),
        )?;
        let running = Arc::clone(&store);
        let task = tokio::spawn(async move {
            running
                .set_attribute(&metadata(&[]), "token", json!("stale"))
                .await
        });
        entered.acquire().await.map_err(|_| failure())?.forget();
        if delete_reinsert {
            store.set_raw_attributes(Map::new())?;
            store.set_raw_attribute("token", json!("initial"))?;
        } else {
            raw_receipt(&store, "committed")?.commit()?;
        }
        release.add_permits(1);
        assert!(
            matches!(task.await.map_err(|_| failure())?, Err(error) if error.code() == "model_attribute_mutation_conflict")
        );
        assert_eq!(
            store.raw_attribute("token")?,
            Some(json!(if delete_reinsert {
                "initial"
            } else {
                "committed"
            }))
        );
        assert_eq!(store.state.read().map_err(|_| failure())?.active_setters, 0);
    }
    Ok(())
}

#[tokio::test]
async fn ordinary_setter_keeps_nonoverlapping_successor_and_detects_accessor_aba() -> Result<()> {
    for replace_accessor in [false, true] {
        let store = Arc::new(ModelAttributes::new(raw(&[("token", json!("initial"))])));
        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let setter: Arc<dyn AttributeSetter> = Arc::new(ConcurrentPrepareSetter {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        });
        store.register_setter("token", Arc::clone(&setter))?;
        let running = Arc::clone(&store);
        let task = tokio::spawn(async move {
            running
                .set_attribute(&metadata(&[]), "token", json!("candidate"))
                .await
        });
        entered.acquire().await.map_err(|_| failure())?.forget();
        store.set_raw_attribute("other", json!(7))?;
        if replace_accessor {
            store.register_setter("token", Arc::new(PatchSetter))?;
            store.register_setter("token", setter)?;
        }
        release.add_permits(1);
        let result = task.await.map_err(|_| failure())?;
        if replace_accessor {
            assert!(
                matches!(result, Err(error) if error.code() == "model_attribute_mutation_conflict")
            );
        } else {
            result?;
            assert_eq!(store.raw_attribute("token")?, Some(json!("candidate")));
        }
        assert_eq!(store.raw_attribute("other")?, Some(json!(7)));
    }
    Ok(())
}

#[test]
fn expired_field_tombstones_are_pruned_without_active_setters() -> Result<()> {
    let store = ModelAttributes::default();
    for index in 0..500 {
        store.set_raw_attributes(Map::from_iter([(format!("field_{index}"), json!(index))]))?;
        assert_eq!(store.state.read().map_err(|_| failure())?.fields.len(), 1);
    }
    store.set_raw_attributes(Map::new())?;
    assert!(store.state.read().map_err(|_| failure())?.fields.is_empty());
    Ok(())
}

#[test]
fn rolling_two_receipts_compress_cancelled_history_and_drop_on_small_stack() -> Result<()> {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| -> Result<()> {
            let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
            let mut current = raw_receipt(&store, "first")?;
            for _ in 0..10_000 {
                let next = raw_receipt(&store, "next")?;
                assert!(!current.rollback()?);
                drop(current);
                current = next;
                let state = store.state.read().map_err(|_| failure())?;
                let mut node = state.owners.get("token").cloned();
                let mut length = 0;
                while let Some(current) = node {
                    length += 1;
                    node = current
                        .history
                        .lock()
                        .map_err(|_| failure())?
                        .predecessor
                        .clone();
                }
                assert!(length <= 1);
                assert!(state.nodes.len() <= 3);
            }
            drop(current);
            assert_eq!(store.raw_attribute("token")?, Some(json!("initial")));
            Ok(())
        })
        .map_err(|_| failure())?
        .join()
        .map_err(|_| failure())??;
    Ok(())
}

#[test]
fn committed_reads_reject_candidates_and_allow_commit_or_restored_value() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut receipt = raw_receipt(&store, "candidate")?;
    assert!(
        matches!(store.committed_raw_attribute("token"), Err(error) if error.code() == "model_attribute_update_pending" && error.status().as_u16() == 409)
    );
    assert!(
        matches!(store.committed_attribute(&metadata(&[]), "other"), Err(error) if error.code() == "model_attribute_update_pending")
    );
    assert_eq!(store.raw_attribute("token")?, Some(json!("candidate")));
    assert!(receipt.rollback()?);
    assert_eq!(
        store.committed_raw_attribute("token")?,
        Some(json!("initial"))
    );
    raw_receipt(&store, "committed")?.commit()?;
    assert_eq!(
        store.committed_attribute(&metadata(&[]), "token")?,
        Some(json!("committed"))
    );
    Ok(())
}

#[test]
fn committed_getter_reentry_rejects_changed_snapshot() -> Result<()> {
    let store = Arc::new(ModelAttributes::new(raw(&[("id", json!("initial"))])));
    store.register_getter("id", Arc::new(ReentrantGetter(Arc::downgrade(&store))))?;
    assert!(
        matches!(store.committed_attribute(&metadata(&[]), "id"), Err(error) if error.code() == "model_attribute_read_conflict" && error.status().as_u16() == 409)
    );
    assert_eq!(store.raw_attribute("id")?, Some(json!("initial")));
    assert_eq!(
        store.raw_attribute("getter_side_effect")?,
        Some(json!(true))
    );
    Ok(())
}

struct ReentrantRawDrop {
    store: Weak<ModelAttributes>,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for ReentrantRawDrop {
    fn drop(&mut self) {
        if let Some(store) = self.store.upgrade() {
            let _ = store.raw_attribute("token");
            self.dropped
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

#[test]
fn raw_update_prechecks_overflow_before_callback_and_drops_reentrant_capture_outside_lock()
-> Result<()> {
    let store = Arc::new(ModelAttributes::default());
    store.state.write().map_err(|_| failure())?.revision = u64::MAX;
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let probe = ReentrantRawDrop {
        store: Arc::downgrade(&store),
        dropped: Arc::clone(&dropped),
    };
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_called = Arc::clone(&called);
    let result = store.update_raw_attributes(move |_| {
        callback_called.store(true, std::sync::atomic::Ordering::Release);
        drop(probe);
        Ok(())
    });
    assert!(matches!(result, Err(error) if error.code() == "model_attribute_revision_exhausted"));
    assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
    assert!(!called.load(std::sync::atomic::Ordering::Acquire));
    assert!(store.raw_attributes()?.is_empty());
    Ok(())
}

#[test]
fn raw_update_return_value_can_reenter_on_drop_after_success() -> Result<()> {
    let store = Arc::new(ModelAttributes::default());
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let probe = ReentrantRawDrop {
        store: Arc::downgrade(&store),
        dropped: Arc::clone(&dropped),
    };
    let result = store.update_raw_attributes(move |values| {
        values.insert("token".into(), json!("new"));
        Ok(probe)
    })?;
    assert_eq!(store.raw_attribute("token")?, Some(json!("new")));
    drop(result);
    assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
    Ok(())
}

#[test]
fn node_last_owner_drops_very_long_chain_iteratively_on_small_stack() -> Result<()> {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let status = Arc::new(std::sync::atomic::AtomicU8::new(super::CANCELLED));
            let mut head = None;
            for _ in 0..20_000 {
                head = Some(Arc::new(super::MutationNode {
                    value: json!("token").into(),
                    status: Arc::clone(&status),
                    history: std::sync::Mutex::new(super::NodeHistory {
                        original: None,
                        predecessor: head,
                    }),
                }));
            }
            drop(head);
        })
        .map_err(|_| failure())?
        .join()
        .map_err(|_| failure())?;
    Ok(())
}

#[test]
fn committed_reads_also_reject_cancelled_owner_before_raw_cleanup() -> Result<()> {
    let store = ModelAttributes::new(raw(&[("token", json!("initial"))]));
    let mut receipt = raw_receipt(&store, "candidate")?;
    // 等价有序状态注入：生产rollback在同一写锁内标Cancelled并清理raw，
    // 读取不能观察中间窗口；此处确认即使保留该状态也不会泄露候选。
    receipt
        .status
        .store(super::CANCELLED, std::sync::atomic::Ordering::Release);
    assert!(
        matches!(store.committed_raw_attribute("token"), Err(error) if error.code() == "model_attribute_update_pending")
    );
    assert!(
        matches!(store.committed_attribute(&metadata(&[]), "token"), Err(error) if error.code() == "model_attribute_update_pending")
    );
    assert!(receipt.rollback()?);
    assert_eq!(
        store.committed_raw_attribute("token")?,
        Some(json!("initial"))
    );
    Ok(())
}

struct LargePatchSetter;
impl AttributeSetter for LargePatchSetter {
    fn set(&self, _: AttributeValue, _: AttributeMap) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async {
            Ok((0..2048)
                .map(|index| {
                    (
                        format!("field_{index}"),
                        AttributeValue::from(json!("secret")),
                    )
                })
                .collect())
        })
    }
}

#[tokio::test]
async fn final_large_patch_terminal_releases_weak_capacity_and_explicit_rollback_nodes()
-> Result<()> {
    for commit in [false, true] {
        let store = ModelAttributes::default();
        store.register_setter("bulk", Arc::new(LargePatchSetter))?;
        let mut receipt = store
            .set_attribute_reversibly_if(&metadata(&[]), "bulk", Value::Null, |_| Ok(true))
            .await?
            .ok_or_else(failure)?;
        let weak_nodes: Vec<_> = receipt.nodes.values().map(Arc::downgrade).collect();
        assert_eq!(weak_nodes.len(), 2048);
        if commit {
            receipt.commit()?;
        } else {
            assert!(receipt.rollback()?);
            assert!(receipt.nodes.is_empty());
            // 保持已rollback收据活着，证明无需等Drop或下一次可回滚操作才释放节点。
            assert!(weak_nodes.iter().all(|node| node.upgrade().is_none()));
        }
        store.set_raw_attributes(Map::new())?;
        let state = store.state.read().map_err(|_| failure())?;
        assert_eq!(state.pending_count, 0);
        assert!(state.nodes.is_empty());
        assert_eq!(state.nodes.capacity(), 0);
        assert!(state.owners.is_empty());
        assert!(state.fields.is_empty());
        assert!(weak_nodes.iter().all(|node| node.upgrade().is_none()));
    }
    Ok(())
}

#[test]
fn date_carrier_preserves_identity_through_original_and_rollback() -> Result<()> {
    use super::super::ModelDate;
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};

    let date = ModelDate::new(
        InputDate::new(
            CalendarInstant::new(10, 123_456)?,
            ResolvedTimezone::parse("Asia/Kolkata")?,
        ),
        false,
    );
    let next = date.copy()?;
    let store = ModelAttributes::from_attribute_values(AttributeMap::from_iter([
        ("before".into(), json!({}).into()),
        ("stamp".into(), date.clone().into()),
        ("after".into(), json!([]).into()),
    ]));
    let metadata = ModelMetadata::new("events", "app::Event");
    let original = store.get_raw_original_value("stamp")?.ok_or_else(failure)?;
    assert!(
        original
            .as_date()
            .is_some_and(|value| value.same_object(&date))
    );
    assert_eq!(store.raw_attribute("before")?, Some(json!({})));
    assert_eq!(store.raw_attribute("after")?, Some(json!([])));
    assert_eq!(
        store.raw_attributes().err().ok_or_else(failure)?.code(),
        "model_attribute_object_projection_required"
    );
    let mut receipt = store
        .set_raw_attribute_value_reversibly_if("stamp", next.clone().into(), |current| {
            Ok(current
                .as_ref()
                .and_then(AttributeValue::as_date)
                .is_some_and(|value| value.same_object(&date)))
        })?
        .ok_or_else(failure)?;
    assert_eq!(
        store
            .committed_raw_attribute_value("stamp")
            .err()
            .ok_or_else(failure)?
            .code(),
        "model_attribute_update_pending"
    );
    assert!(
        store
            .get_attribute_value(&metadata, "stamp")?
            .as_ref()
            .and_then(AttributeValue::as_date)
            .is_some_and(|value| value.same_object(&next))
    );
    assert!(receipt.rollback()?);
    assert!(store.is_clean(&metadata, &[])?);
    assert!(
        store
            .raw_attribute_value("stamp")?
            .as_ref()
            .and_then(AttributeValue::as_date)
            .is_some_and(|value| value.same_object(&date))
    );
    assert_eq!(
        store
            .raw_attribute_values()?
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["before", "stamp", "after"],
    );
    store.set_raw_attribute_value("stamp", next.clone().into())?;
    store.sync_changes(&metadata)?;
    let changes = store.get_changes_values()?;
    let previous = store.get_previous_values()?;
    assert!(
        changes["stamp"]
            .as_date()
            .is_some_and(|value| value.same_object(&next))
    );
    assert!(
        previous["stamp"]
            .as_date()
            .is_some_and(|value| value.same_object(&date))
    );
    assert!(store.was_changed(&["stamp"])?);
    store.discard_changes()?;
    assert!(store.is_clean(&metadata, &[])?);
    Ok(())
}

#[test]
fn model_date_keeps_mutable_aliases_and_rejects_outdated_updates() -> Result<()> {
    use super::super::ModelDate;
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};

    let input = InputDate::new(CalendarInstant::new(10, 123_456)?, ResolvedTimezone::utc());
    let replacement = InputDate::new(
        CalendarInstant::new(11, 654_321)?,
        ResolvedTimezone::parse("+05:30")?,
    );
    let date = ModelDate::new(input.clone(), false);
    let alias = date.clone();
    let stale = date.modify(|previous| {
        let updated = alias.modify(|_| Ok(replacement.clone()))?;
        assert!(updated.same_object(&date));
        Ok(previous)
    });
    assert_eq!(
        stale.err().ok_or_else(failure)?.code(),
        "model_date_update_conflict"
    );
    assert_eq!(date.snapshot()?.instant(), replacement.instant());
    assert_eq!(date.snapshot()?.format("P")?, "+05:30");

    let immutable = ModelDate::new(input, true);
    let updated = immutable.modify(|_| Ok(replacement.clone()))?;
    assert!(updated.is_immutable());
    assert!(!updated.same_object(&immutable));
    assert_eq!(immutable.snapshot()?.timestamp(), 10);
    assert_eq!(updated.snapshot()?.instant(), replacement.instant());
    Ok(())
}

#[tokio::test]
async fn marked_date_accessor_reuses_identity_and_writes_back_multiple_fields() -> Result<()> {
    use super::super::ModelDate;
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let reads = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    let read_count = Arc::clone(&reads);
    let write_count = Arc::clone(&writes);
    let store = ModelAttributes::new(raw(&[("stamp", json!(1_710_057_600_i64))]));
    let metadata = ModelMetadata::new("events", "Event");
    store.register_attribute(
        "stamp",
        ModelAttribute::new()
            .get(move |raw, _| {
                read_count.fetch_add(1, Ordering::Relaxed);
                let seconds = raw
                    .ok_or_else(failure)?
                    .as_json()?
                    .as_i64()
                    .ok_or_else(failure)?;
                Ok(ModelDate::new(
                    InputDate::new(
                        CalendarInstant::new(seconds, 123_456)?,
                        ResolvedTimezone::utc(),
                    ),
                    false,
                )
                .into())
            })
            .set(move |date, _| {
                write_count.fetch_add(1, Ordering::Relaxed);
                let stamp = date.as_date().ok_or_else(failure)?.snapshot()?.timestamp();
                Ok(AttributeMap::from_iter([
                    ("stamp".into(), json!(stamp).into()),
                    ("epoch".into(), json!(stamp).into()),
                ]))
            }),
    )?;
    let first = store
        .get_attribute_value(&metadata, "stamp")?
        .ok_or_else(failure)?;
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    let second = store
        .get_attribute_value(&metadata, "stamp")?
        .ok_or_else(failure)?;
    let first = first.as_date().ok_or_else(failure)?;
    assert!(
        second
            .as_date()
            .is_some_and(|value| value.same_object(first))
    );
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert_eq!(writes.load(Ordering::Relaxed), 1);
    first.modify(|date| {
        Ok(InputDate::new(
            CalendarInstant::new(date.timestamp() + 86_400, date.microseconds())?,
            date.timezone().clone(),
        ))
    })?;
    assert_eq!(
        store.get_dirty(&metadata)?,
        raw(&[
            ("stamp", json!(1_710_144_000_i64)),
            ("epoch", json!(1_710_144_000_i64))
        ]),
    );
    assert_eq!(writes.load(Ordering::Relaxed), 2);
    assert_eq!(
        store.get_raw_original("stamp")?,
        Some(json!(1_710_057_600_i64))
    );
    let explicit = first.copy()?;
    store
        .set_attribute_value(&metadata, "stamp", explicit.clone().into())
        .await?;
    let cached = store
        .get_attribute_value(&metadata, "stamp")?
        .ok_or_else(failure)?;
    assert!(
        cached
            .as_date()
            .is_some_and(|value| value.same_object(&explicit))
    );

    store.set_raw_attributes(raw(&[("stamp", json!(10))]))?;
    let after = store
        .get_attribute_value(&metadata, "stamp")?
        .ok_or_else(failure)?;
    assert!(!after.as_date().ok_or_else(failure)?.same_object(&explicit));
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    Ok(())
}

#[test]
fn marked_cache_writeback_does_not_override_reentrant_raw_changes() -> Result<()> {
    use super::super::ModelDate;
    use crate::support::{CalendarInstant, InputDate, ResolvedTimezone};

    let store = Arc::new(ModelAttributes::new(raw(&[("stamp", json!(10))])));
    let weak = Arc::downgrade(&store);
    let metadata = ModelMetadata::new("events", "Event");
    store.register_attribute(
        "stamp",
        ModelAttribute::new()
            .get(|_, _| {
                Ok(ModelDate::new(
                    InputDate::new(CalendarInstant::new(10, 0)?, ResolvedTimezone::utc()),
                    false,
                )
                .into())
            })
            .set(move |_, _| {
                let store = weak.upgrade().ok_or_else(failure)?;
                store.set_raw_attribute("stamp", json!(30))?;
                Ok(AttributeMap::from_iter([
                    ("stamp".into(), json!(20).into()),
                    ("epoch".into(), json!(20).into()),
                ]))
            }),
    )?;
    assert!(store.get_attribute_value(&metadata, "stamp")?.is_some());
    assert_eq!(
        store.get_dirty(&metadata).err().ok_or_else(failure)?.code(),
        "model_attribute_mutation_conflict"
    );
    assert_eq!(store.raw_attribute("stamp")?, Some(json!(30)));
    assert_eq!(store.raw_attribute("epoch")?, None);
    Ok(())
}
