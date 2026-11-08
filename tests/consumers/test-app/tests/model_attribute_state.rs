//! 外部package只使用公开属性状态API；不调用数据库或内部State。
use openrust::{
    Result,
    database::eloquent::{
        AttributeCast, AttributeGetter, AttributeMap, AttributeValue, ModelAttributes,
        ModelMetadata,
    },
};
use serde_json::{Map, Value, json};

#[test]
fn public_attribute_state_supports_paths_casts_changes_and_fluent_sync() -> Result<()> {
    let mut metadata = ModelMetadata::new("external_records", "external::Record");
    metadata
        .casts
        .insert("options".into(), AttributeCast::Array);
    let attributes = ModelAttributes::new(Map::from_iter([
        ("id".into(), json!("00042")),
        ("options".into(), json!("{}")),
        ("nested".into(), json!({"nullable":null})),
    ]));
    assert!(attributes.is_clean(&metadata, &[])?);
    assert_eq!(
        attributes.get_raw_original_or("nested.nullable", json!("fallback"))?,
        Value::Null
    );
    attributes.set_raw_attribute("id", json!(42))?;
    attributes.set_raw_attribute("options", json!("[]"))?;
    assert!(attributes.is_clean(&metadata, &[])?);
    attributes.set_raw_attribute("name", json!("external"))?;
    let synced = attributes.sync_changes(&metadata)?;
    assert!(std::ptr::eq(synced, &attributes));
    assert_eq!(synced.get_changes()?.get("name"), Some(&json!("external")));
    assert!(synced.get_previous()?.is_empty());
    assert!(synced.was_changed(&["name"])?);
    assert!(synced.is_dirty(&metadata, &[])?);
    synced
        .sync_original_attribute("name")?
        .sync_original_attributes(&["id", "options"])?
        .sync_original()?;
    assert!(synced.is_clean(&metadata, &[])?);
    assert_eq!(synced.get_raw_original("id")?, Some(json!(42)));
    Ok(())
}

#[test]
fn metadata_cast_extension_is_public_and_preserves_read_phase_checks() -> Result<()> {
    let mut metadata = ModelMetadata::new("external_records", "external::Record");
    metadata.key_type = "hashed".into();
    assert_eq!(
        metadata.cast_attribute("id", Some(json!("hash")))?,
        Some(json!("hash"))
    );
    metadata.key_type = " HaShEd ".into();
    assert!(
        matches!(metadata.cast_attribute("id", Some(json!("hash"))), Err(error) if error.code() == "model_attribute_cast_unsupported" && error.status().as_u16() == 500)
    );
    assert_eq!(
        metadata.cast_attribute("id", Some(Value::Null))?,
        Some(Value::Null)
    );
    metadata.key_type = " INT ".into();
    assert_eq!(
        metadata.cast_attribute("id", Some(json!("01")))?,
        Some(json!(1))
    );
    metadata.key_type = "unsupported".into();
    assert!(
        matches!(metadata.cast_attribute("id", None), Err(error) if error.code() == "model_attribute_cast_unsupported")
    );
    Ok(())
}

struct SavedLabel;

impl AttributeGetter for SavedLabel {
    fn get(
        &self,
        value: Option<&AttributeValue>,
        original: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        let label = value.map(AttributeValue::as_json).transpose()?;
        let locale = original
            .get("locale")
            .map(AttributeValue::as_json)
            .transpose()?;
        Ok(Some(json!({"label": label, "locale": locale}).into()))
    }
}

#[test]
fn public_original_projection_and_discard_use_the_same_baseline() -> Result<()> {
    let metadata = ModelMetadata::new("external_records", "external::Record");
    let attributes = ModelAttributes::new(Map::from_iter([
        ("id".into(), json!("00042")),
        ("label".into(), json!("saved")),
        ("locale".into(), json!("zh-CN")),
    ]));
    attributes.set_raw_attribute("label", json!("draft"))?;
    attributes.set_raw_attribute("locale", json!("en"))?;
    attributes.set_raw_attribute("added", Value::Null)?;
    attributes.sync_changes(&metadata)?;
    assert_eq!(attributes.get_changes()?.len(), 3);
    assert_eq!(
        attributes.get_previous()?.get("locale"),
        Some(&json!("zh-CN"))
    );

    let getter = SavedLabel;
    let select = |name: &str| (name == "label").then_some(&getter as &dyn AttributeGetter);
    let original = attributes.get_original_attributes(&metadata, select)?;
    assert_eq!(original.get("id"), Some(&json!(42)));
    assert_eq!(
        attributes.get_original(&metadata, "label", None, select)?,
        Some(json!({"label":"saved", "locale":"zh-CN"}))
    );
    assert!(!original.contains_key("added"));

    let restored = attributes.discard_changes()?;
    assert!(std::ptr::eq(restored, &attributes));
    assert_eq!(restored.raw_attribute("id")?, Some(json!("00042")));
    assert_eq!(restored.raw_attribute("locale")?, Some(json!("zh-CN")));
    assert_eq!(restored.raw_attribute("added")?, None);
    assert_eq!(
        restored.get_original_attributes(&metadata, select)?,
        original
    );
    assert!(restored.get_changes()?.is_empty());
    assert!(restored.get_previous()?.is_empty());
    assert!(restored.is_clean(&metadata, &[])?);
    Ok(())
}
