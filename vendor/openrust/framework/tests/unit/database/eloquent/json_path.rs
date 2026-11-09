use super::*;
use crate::database::eloquent::{ArrayObject, AsArrayObject, MassAssignment, ModelAttributes};
use serde_json::json;

fn model() -> ModelMetadata {
    let mut model = ModelMetadata::new("records", "Record");
    model.casts.insert("bag".into(), AttributeCast::Array);
    model
}

fn raw(json: &str) -> ModelAttributes {
    ModelAttributes::new(Map::from_iter([("bag".into(), json!(json))]))
}

#[tokio::test]
async fn paths_preserve_order_sparse_keys_literal_raw_and_full_key_cast_priority() -> Result<()> {
    let mut model = model();
    let attrs = raw(r#"{"a.b":1,"2":"old","02":"literal"}"#);
    attrs.set_raw_attribute("bag->a.b", json!("raw literal key"))?;
    for (path, value) in [
        ("bag->a.b", json!(7)),
        ("bag->4->x", json!(8)),
        ("bag->", json!("empty")),
        ("bag->dot..tail", json!(3)),
        ("bag->*", json!("star")),
    ] {
        attrs.set_attribute(&model, path, value).await?;
    }
    assert_eq!(
        attrs.raw_attribute("bag")?,
        Some(json!(
            r#"{"a.b":1,"2":"old","02":"literal","a":{"b":7},"4":{"x":8},"":"empty","dot":{"":{"tail":3}},"*":"star"}"#
        ))
    );
    assert_eq!(
        attrs.raw_attribute("bag->a.b")?,
        Some(json!("raw literal key"))
    );
    model.casts.insert("bag->key".into(), AttributeCast::Array);
    attrs
        .set_attribute(&model, "bag->key", json!({"x":1}))
        .await?;
    assert_eq!(
        attrs
            .get_attribute(&model, "bag")?
            .and_then(|bag| bag.get("key").cloned()),
        Some(json!(r#"{"x":1}"#))
    );

    let original = attrs.raw_attribute("bag")?;
    model.casts.insert("bag->key".into(), AsArrayObject::cast());
    attrs
        .set_attribute(&model, "bag->key", json!({"x":2}))
        .await?;
    assert_eq!(attrs.raw_attribute("bag")?, original);
    assert_eq!(attrs.raw_attribute("bag->key")?, Some(json!(r#"{"x":2}"#)));
    Ok(())
}

#[tokio::test]
async fn path_roots_follow_php_scalars_and_failed_writes_keep_original_raw() -> Result<()> {
    let model = model();
    for root in [
        None,
        Some(Value::Null),
        Some(json!("null")),
        Some(json!("false")),
        Some(json!("invalid")),
        Some(json!("")),
    ] {
        let attrs = ModelAttributes::default();
        if let Some(root) = root {
            attrs.set_raw_attribute("bag", root)?;
        }
        attrs
            .set_attribute(&model, "bag->new->leaf", json!(9))
            .await?;
        assert_eq!(
            attrs.raw_attribute("bag")?,
            Some(json!(r#"{"new":{"leaf":9}}"#))
        );
    }
    for root in ["true", "4", "4.5", r#""abc""#] {
        let attrs = raw(root);
        assert!(
            attrs
                .set_attribute(&model, "bag->new", json!(9))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("bag")?, Some(json!(root)));
    }
    for (key, expected) in [("0", "9bc"), ("-1", "ab9"), ("5", "abc  9"), ("01", "a9c")] {
        let attrs = raw(r#""abc""#);
        attrs
            .set_attribute(&model, &format!("bag->{key}"), json!(9))
            .await?;
        assert_eq!(attrs.get_attribute(&model, "bag")?, Some(json!(expected)));
    }
    let attrs = raw(r#""文字""#);
    assert!(
        attrs
            .set_attribute(&model, "bag->1", json!("x"))
            .await
            .is_err()
    );
    assert!(
        attrs
            .set_attribute(&model, "bag->0", json!("xy"))
            .await
            .is_err()
    );
    assert!(
        attrs
            .set_attribute(&model, "bag->999999999999", json!("x"))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn unicode_path_applies_full_key_and_base_flags_independently() -> Result<()> {
    let mut model = model();
    let attrs = raw(r#"{"old":"中文"}"#);
    model.casts.insert("bag".into(), AttributeCast::JsonUnicode);
    attrs
        .set_attribute(&model, "bag->value", json!("汉字😀/"))
        .await?;
    assert_eq!(
        attrs.raw_attribute("bag")?,
        Some(json!(r#"{"old":"中文","value":"汉字😀\/"}"#))
    );

    model.casts.insert("bag".into(), AttributeCast::Object);
    model
        .casts
        .insert("bag->value".into(), AttributeCast::JsonUnicode);
    attrs
        .set_attribute(&model, "bag->value", json!("汉字😀/"))
        .await?;
    assert_eq!(
        attrs.raw_attribute("bag")?,
        Some(json!(
            r#"{"old":"\u4e2d\u6587","value":"\"\u6c49\u5b57\ud83d\ude00\\\/\""}"#
        ))
    );
    assert_eq!(
        attrs
            .get_attribute(&model, "bag")?
            .and_then(|bag| bag.get("value").cloned()),
        Some(json!(r#""汉字😀\/""#))
    );
    Ok(())
}

#[tokio::test]
async fn paths_flush_then_invalidate_objects_and_respect_fillable_and_rollback() -> Result<()> {
    let mut model = model();
    model.casts.insert("bag".into(), AsArrayObject::cast());
    let attrs = raw(r#"{"before":1}"#);
    let read = || {
        attrs
            .get_attribute_value(&model, "bag")?
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(path_error)
    };
    let previous = read()?;
    previous.modify(|value: &ArrayObject| {
        let mut next = value.clone();
        next.as_collection_mut().try_put("edited", json!(2))?;
        Ok(next)
    })?;
    attrs
        .set_attribute(&model, "bag->nested->x", json!(3))
        .await?;
    assert!(!previous.same_object(&read()?));
    assert_eq!(
        attrs.raw_attribute("bag")?,
        Some(json!(r#"{"before":1,"edited":2,"nested":{"x":3}}"#))
    );
    assert_eq!(
        attrs.get_raw_original("bag")?,
        Some(json!(r#"{"before":1}"#))
    );
    let current = read()?;
    let bad = format!(
        "bag->{}",
        std::iter::repeat_n("x", 513).collect::<Vec<_>>().join("->")
    );
    assert!(attrs.set_attribute(&model, &bad, json!(4)).await.is_err());
    assert!(current.same_object(&read()?));
    model.fillable(vec!["bag->allowed".into()]);
    attrs
        .fill(
            &model,
            Map::from_iter([
                ("bag->allowed".into(), json!(4)),
                ("bag->blocked".into(), json!(5)),
            ]),
            &MassAssignment::default(),
        )
        .await?;
    assert!(
        attrs
            .raw_attribute("bag")?
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains(r#""allowed":4"#) && !text.contains("blocked"))
    );
    let before = attrs.raw_attribute("bag")?;
    let mut receipt = attrs
        .set_attribute_reversibly_if(&model, "bag->temporary", json!(1), |_| Ok(true))
        .await?
        .ok_or_else(path_error)?;
    assert!(receipt.rollback()?);
    assert_eq!(attrs.raw_attribute("bag")?, before);
    Ok(())
}

#[tokio::test]
async fn paths_flush_only_the_base_column_cache() -> Result<()> {
    use crate::{
        database::eloquent::{AttributeMap, ClassCast, ClassCastWrite, ClassCaster, ModelObject},
        support::Collection,
    };
    struct FailingCast;
    impl ClassCaster for FailingCast {
        fn get_value(
            &self,
            _: &str,
            _: Option<&AttributeValue>,
            _: &AttributeMap,
        ) -> Result<AttributeValue> {
            Ok(ModelObject::new(ArrayObject::new(Collection::from_value(json!([]))?)).into())
        }

        fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
            if value.as_object().is_none() {
                return Ok(ClassCastWrite::Value(value));
            }
            Err(Error::custom(
                "test_unrelated_cast_write",
                "不相关的回写被执行",
            ))
        }
    }
    let mut model = model();
    model.casts.insert(
        "other".into(),
        AttributeCast::Class(ClassCast::new(|| Ok(FailingCast))),
    );
    let attrs = raw("{}");
    assert!(attrs.get_attribute_value(&model, "other")?.is_some());
    attrs.set_attribute(&model, "bag->nested", json!(3)).await?;
    assert_eq!(attrs.raw_attribute("bag")?, Some(json!(r#"{"nested":3}"#)));
    let mut receipt = attrs
        .set_attribute_reversibly_if(&model, "bag->nested", json!(4), |_| Ok(true))
        .await?
        .ok_or_else(path_error)?;
    assert!(receipt.rollback()?);
    assert_eq!(attrs.raw_attribute("bag")?, Some(json!(r#"{"nested":3}"#)));
    model.casts.insert(
        "bag->literal".into(),
        AttributeCast::Class(ClassCast::new(|| Ok(FailingCast))),
    );
    assert!(attrs.get_attribute_value(&model, "bag->literal")?.is_some());
    attrs
        .set_attribute(&model, "bag->literal", json!(7))
        .await?;
    assert_eq!(attrs.raw_attribute("bag->literal")?, Some(json!(7)));
    assert_eq!(attrs.raw_attribute("bag")?, Some(json!(r#"{"nested":3}"#)));
    assert!(attrs.raw_attribute("other").is_err());
    Ok(())
}

#[tokio::test]
async fn path_write_rejects_cached_object_changes_during_projection() -> Result<()> {
    use crate::{database::eloquent::ModelObject, support::DataObject};
    use std::any::Any;

    struct ReentrantValue(ModelObject);

    impl DataObject for ReentrantValue {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn json_projection(&self) -> Option<Value> {
            self.0
                .modify(|value: &ArrayObject| {
                    let mut next = value.clone();
                    next.as_collection_mut().try_put("edited", json!(2))?;
                    Ok(next)
                })
                .ok()?;
            Some(json!(3))
        }
    }

    let mut model = model();
    model.casts.insert("bag".into(), AsArrayObject::cast());
    let attrs = raw(r#"{"before":1}"#);
    let cached = attrs
        .get_attribute_value(&model, "bag")?
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(path_error)?;
    let result = attrs
        .set_attribute_value(
            &model,
            "bag->nested",
            ModelObject::new(ReentrantValue(cached)).into(),
        )
        .await;
    assert!(matches!(result, Err(error) if error.code() == "model_attribute_mutation_conflict"));
    assert_eq!(
        attrs.raw_attribute("bag")?,
        Some(json!(r#"{"before":1,"edited":2}"#))
    );
    Ok(())
}

#[cfg(feature = "encryption")]
#[tokio::test]
async fn encrypted_paths_rewrite_full_plaintext_with_current_key_and_reject_bad_payload()
-> Result<()> {
    use crate::{
        config::AppConfig,
        database::eloquent::{
            AsEncryptedArrayObject, AsEncryptedCollection, EncryptedCast, EncryptedCastKind,
        },
        encryption::{ApplicationEncrypter, ConfiguredEncrypter},
    };
    use std::sync::Arc;
    let old_key = "0123456789abcdef0123456789abcdef";
    let new_key = "abcdef0123456789abcdef0123456789";
    let make = |key: &str, previous_keys| {
        ConfiguredEncrypter::from_config(AppConfig {
            key: Some(key.into()),
            previous_keys,
            ..AppConfig::default()
        })
    };
    let old = make(old_key, vec![])?;
    let current = make(new_key, vec![])?;
    let rotated: Arc<dyn ApplicationEncrypter> = Arc::new(make(new_key, vec![old_key.into()])?);
    for cast in [
        AttributeCast::Encrypted(EncryptedCast::new(
            EncryptedCastKind::String,
            rotated.clone(),
        )),
        AttributeCast::Encrypted(EncryptedCast::new(
            EncryptedCastKind::Array,
            rotated.clone(),
        )),
        AsEncryptedArrayObject::cast(rotated.clone()),
        AsEncryptedCollection::cast(rotated),
    ] {
        let mut model = model();
        model.casts.insert("bag".into(), cast);
        let wire = old.encrypt_string(br#"{"before":1}"#)?;
        let attrs = raw(&wire);
        attrs
            .set_attribute(&model, "bag->child->name", json!("updated"))
            .await?;
        let wire = attrs.raw_attribute("bag")?.ok_or_else(path_error)?;
        assert_eq!(
            current.decrypt_string(wire.as_str().ok_or_else(path_error)?)?,
            br#"{"before":1,"child":{"name":"updated"}}"#
        );
        attrs.set_raw_attribute("bag", json!("bad ciphertext"))?;
        assert!(
            attrs
                .set_attribute(&model, "bag->child", json!(1))
                .await
                .is_err()
        );
        assert_eq!(attrs.raw_attribute("bag")?, Some(json!("bad ciphertext")));
    }
    Ok(())
}
