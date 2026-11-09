use super::*;
use crate::{
    config::AppConfig,
    database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata},
    encryption::ConfiguredEncrypter,
};
use serde_json::{Map, json};

const OLD_KEY: &str = "0123456789abcdef0123456789abcdef";
const NEW_KEY: &str = "abcdef0123456789abcdef0123456789";

fn key(value: &str, previous_keys: Vec<String>) -> Result<Arc<dyn ApplicationEncrypter>> {
    Ok(Arc::new(ConfiguredEncrypter::from_config(AppConfig {
        key: Some(value.into()),
        previous_keys,
        ..AppConfig::default()
    })?))
}

fn metadata(kind: EncryptedCastKind, key: &Arc<dyn ApplicationEncrypter>) -> ModelMetadata {
    let mut metadata = ModelMetadata::new("records", "id");
    metadata.casts.insert(
        "data".into(),
        AttributeCast::Encrypted(EncryptedCast::new(kind, Arc::clone(key))),
    );
    metadata
}

fn missing_attribute() -> Error {
    Error::custom("test_attribute_missing", "测试属性缺失")
}

fn store(raw: Value) -> ModelAttributes {
    ModelAttributes::new(Map::from_iter([("data".into(), raw)]))
}

#[tokio::test]
async fn native_scalar_cast_uses_application_key_rotation_and_php_scalar_text() -> Result<()> {
    let old = key(OLD_KEY, vec![])?;
    let current = key(NEW_KEY, vec![])?;
    let rotated = key(NEW_KEY, vec![OLD_KEY.into()])?;
    let model = metadata(EncryptedCastKind::String, &rotated);
    let attrs = store(json!(old.encrypt_string(b"old")?));
    assert_eq!(attrs.get_attribute(&model, "data")?, Some(json!("old")));
    assert!(attrs.original_is_equivalent(&model, "data")?);
    attrs.set_attribute(&model, "data", json!("old")).await?;
    assert!(attrs.is_dirty(&model, &["data"])?);
    let wire = attrs.raw_attribute("data")?.ok_or_else(missing_attribute)?;
    assert_eq!(
        current.decrypt_string(wire.as_str().unwrap_or_default())?,
        b"old"
    );
    attrs.set_raw_attribute("data", json!("bad ciphertext"))?;
    assert!(attrs.is_dirty(&model, &["data"])?);
    assert!(attrs.get_attribute(&model, "data").is_err());

    for (input, expected) in [
        (json!(true), "1"),
        (json!(false), ""),
        (json!(24), "24"),
        (json!(1.0), "1"),
    ] {
        attrs.set_attribute(&model, "data", input).await?;
        assert_eq!(attrs.get_attribute(&model, "data")?, Some(json!(expected)));
    }
    let raw = attrs.raw_attribute("data")?;
    assert!(
        attrs
            .set_attribute(&model, "data", json!([1]))
            .await
            .is_err()
    );
    assert_eq!(attrs.raw_attribute("data")?, raw);
    attrs.set_attribute(&model, "data", Value::Null).await?;
    assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
    assert_eq!(attrs.get_attribute(&model, "data")?, Some(Value::Null));
    Ok(())
}

#[tokio::test]
async fn native_json_variants_share_associative_dirty_comparison_and_keep_read_shape() -> Result<()>
{
    let key = key(NEW_KEY, vec![])?;
    for kind in [
        EncryptedCastKind::Array,
        EncryptedCastKind::Json,
        EncryptedCastKind::Object,
        EncryptedCastKind::Collection,
    ] {
        let model = metadata(kind, &key);
        let attrs = store(json!(key.encrypt_string(br#"{"0":1}"#)?));
        let expected = if kind == EncryptedCastKind::Object {
            json!({"0":1})
        } else {
            json!([1])
        };
        assert_eq!(
            attrs.attributes_to_json(&model)?.get("data"),
            Some(&expected)
        );
        attrs.set_raw_attribute("data", json!(key.encrypt_string(b"[ 1 ]")?))?;
        assert!(attrs.original_is_equivalent(&model, "data")?);
        attrs.set_raw_attribute("data", json!(key.encrypt_string(b"[1.0]")?))?;
        assert!(!attrs.original_is_equivalent(&model, "data")?);
        attrs
            .set_attribute(&model, "data", json!({"字":"a/b"}))
            .await?;
        let wire = attrs.raw_attribute("data")?.ok_or_else(missing_attribute)?;
        assert_eq!(
            key.decrypt_string(wire.as_str().unwrap_or_default())?,
            br#"{"\u5b57":"a\/b"}"#
        );
        attrs.set_attribute(&model, "data", Value::Null).await?;
        assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
        attrs.sync_original()?;
        attrs.set_raw_attribute("data", json!(key.encrypt_string(b"bad json")?))?;
        let (expected, equivalent) = if kind == EncryptedCastKind::Collection {
            (json!([]), false)
        } else {
            (Value::Null, true)
        };
        assert_eq!(
            attrs.attributes_to_json(&model)?.get("data"),
            Some(&expected)
        );
        assert_eq!(attrs.original_is_equivalent(&model, "data")?, equivalent);
    }
    Ok(())
}

#[test]
fn encrypted_object_rejects_nul_property_names_without_changing_array_comparison() -> Result<()> {
    let key = key(NEW_KEY, vec![])?;
    let payload = br#"{"nested":{"\u0000name":1}}"#;
    let attrs = store(json!(key.encrypt_string(payload)?));
    let object = metadata(EncryptedCastKind::Object, &key);
    let array = metadata(EncryptedCastKind::Array, &key);
    assert_eq!(attrs.get_attribute(&object, "data")?, Some(Value::Null));
    assert_eq!(
        attrs.get_attribute(&array, "data")?,
        Some(json!({"nested":{"\0name":1}}))
    );
    attrs.set_raw_attribute("data", json!(key.encrypt_string(payload)?))?;
    assert!(attrs.original_is_equivalent(&object, "data")?);
    Ok(())
}

#[test]
fn native_collection_reads_fresh_typed_objects_and_serialize_through_model_projection() -> Result<()>
{
    let key = key(NEW_KEY, vec![])?;
    let model = metadata(EncryptedCastKind::Collection, &key);
    let attrs = store(json!(key.encrypt_string(b"7")?));
    let read = || {
        attrs
            .get_attribute_value(&model, "data")?
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(missing_attribute)
    };
    let first = read()?;
    first.modify(|collection: &Collection| {
        let mut changed = collection.clone();
        changed.try_put(0, json!(8))?;
        Ok(changed)
    })?;
    assert_eq!(first.project_json()?, json!([8]));
    assert_eq!(read()?.project_json()?, json!([7]));
    assert_eq!(
        attrs.attributes_to_json(&model)?.get("data"),
        Some(&json!([7]))
    );
    assert!(attrs.is_clean(&model, &["data"])?);
    Ok(())
}
