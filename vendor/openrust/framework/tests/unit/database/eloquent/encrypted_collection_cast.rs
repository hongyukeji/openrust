use super::*;
use crate::{
    config::AppConfig,
    database::eloquent::{ModelAttributes, ModelMetadata},
    encryption::ConfiguredEncrypter,
};
use serde_json::{Map, json};

const OLD_KEY: &str = "0123456789abcdef0123456789abcdef";
const NEW_KEY: &str = "abcdef0123456789abcdef0123456789";

fn service(key: &str, previous_keys: Vec<String>) -> Result<Arc<dyn ApplicationEncrypter>> {
    Ok(Arc::new(ConfiguredEncrypter::from_config(AppConfig {
        key: Some(key.into()),
        previous_keys,
        ..AppConfig::default()
    })?))
}

fn model(cast: AttributeCast) -> ModelMetadata {
    let mut model = ModelMetadata::new("records", "Record");
    model.casts.insert("data".into(), cast);
    model
}

fn store(value: Value) -> ModelAttributes {
    ModelAttributes::new(Map::from_iter([("data".into(), value)]))
}

#[tokio::test]
async fn encrypted_object_reads_history_and_rewrites_with_current_key() -> Result<()> {
    let old = service(OLD_KEY, Vec::new())?;
    let current = service(NEW_KEY, Vec::new())?;
    let rotated = service(NEW_KEY, vec![OLD_KEY.into()])?;
    let previous = old.encrypt_string(br#"{"tag":"old"}"#)?;
    let model = model(AsEncryptedArrayObject::cast(Arc::clone(&rotated)));
    let attributes = store(json!(previous));
    assert!(attributes.original_is_equivalent(&model, "data")?);
    attributes.set_raw_attribute("data", json!(current.encrypt_string(br#"{"tag":"old"}"#)?))?;
    assert!(!attributes.original_is_equivalent(&model, "data")?);
    attributes.set_raw_attribute("data", json!("bad ciphertext"))?;
    assert!(!attributes.original_is_equivalent(&model, "data")?);
    attributes.set_raw_attribute("data", json!(old.encrypt_string(br#"{"tag":"old"}"#)?))?;
    let value = attributes.get_attribute_value(&model, "data")?;
    let object = value
        .as_ref()
        .and_then(AttributeValue::as_object)
        .ok_or_else(|| Error::custom("test_encrypted_array_missing", "缺少已解密对象"))?;
    object.modify(|value: &ArrayObject| {
        let mut next = value.clone();
        next.as_collection_mut().try_put("tag", json!("changed"))?;
        Ok(next)
    })?;

    let dirty = attributes.get_dirty(&model)?;
    let payload = dirty["data"]
        .as_str()
        .ok_or_else(|| Error::custom("test_encrypted_payload", "缺少回写密文"))?;
    assert_eq!(current.decrypt_string(payload)?, br#"{"tag":"changed"}"#);
    assert!(old.decrypt_string(payload).is_err());
    assert_eq!(
        attributes.attributes_to_json(&model)?["data"],
        json!({"tag":"changed"})
    );
    attributes
        .set_attribute(&model, "data", Value::Null)
        .await?;
    assert_eq!(attributes.raw_attribute("data")?, Some(Value::Null));
    Ok(())
}

#[test]
fn encrypted_collection_compares_plaintext_only_without_history_and_maps_scalars() -> Result<()> {
    let encrypter = service(NEW_KEY, Vec::new())?;
    let model = model(AsEncryptedCollection::using(
        Arc::clone(&encrypter),
        AsCollection::of(|value, key| Ok((key.to_string(), value))),
    ));
    let attributes = store(json!(encrypter.encrypt_string(br#"{"0":1}"#)?));
    attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(br#"{"0":1}"#)?))?;
    assert!(attributes.original_is_equivalent(&model, "data")?);
    attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"[1]")?))?;
    assert!(!attributes.original_is_equivalent(&model, "data")?);
    attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"7")?))?;
    assert_eq!(
        attributes.attributes_to_array(&model)?["data"].as_json()?,
        &json!([["0", 7]])
    );
    attributes.set_raw_attribute("data", json!(encrypter.encrypt_string(b"bad json")?))?;
    assert_eq!(attributes.attributes_to_json(&model)?["data"], json!([]));
    attributes.set_raw_attribute("data", json!("not ciphertext"))?;
    assert!(attributes.get_attribute_value(&model, "data").is_err());
    assert!(attributes.original_is_equivalent(&model, "data").is_err());
    Ok(())
}

#[test]
fn encrypted_array_object_rejects_non_array_plaintext() -> Result<()> {
    let encrypter = service(NEW_KEY, Vec::new())?;
    let model = model(AsEncryptedArrayObject::cast(Arc::clone(&encrypter)));
    let attributes = store(json!(encrypter.encrypt_string(b"7")?));
    assert!(matches!(
        attributes.get_attribute_value(&model, "data"),
        Err(error) if error.code()=="model_encrypted_array_object_shape"
    ));
    Ok(())
}
