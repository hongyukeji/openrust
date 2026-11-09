//! 模型的枚举数组对象和集合转换。
use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster, EnumCase,
    EnumCast, Json, ModelObject,
};
use crate::{
    Error, Result,
    support::{Collection, DataObject},
};
use serde_json::Value;
use std::any::Any;

/// 保留有序键和类型化 case 的可变枚举数组对象。
#[derive(Clone)]
pub struct EnumArrayObject(Collection<EnumCase>);

impl EnumArrayObject {
    /// 从有序枚举元素创建数组对象。
    pub const fn new(values: Collection<EnumCase>) -> Self {
        Self(values)
    }

    /// 借用当前有序枚举元素。
    pub const fn as_collection(&self) -> &Collection<EnumCase> {
        &self.0
    }

    /// 在独立快照上修改有序枚举元素。
    pub const fn as_collection_mut(&mut self) -> &mut Collection<EnumCase> {
        &mut self.0
    }
}

impl DataObject for EnumArrayObject {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn json_projection(&self) -> Option<Value> {
        self.0.json_projection()
    }
}

// 追加字段和同名访问器绕过 caster 的显式序列化，保留 Arrayable 返回的 case。
struct EnumCaseArray(Collection<EnumCase>);

impl DataObject for EnumCaseArray {
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn json_projection(&self) -> Option<Value> {
        if self.0.iter().any(|(_, case)| !case.has_backing()) {
            return None;
        }
        self.0.json_projection()
    }
}

pub(super) fn project_enum_array(value: AttributeValue) -> Result<AttributeValue> {
    let Some(object) = value.as_object() else {
        return Ok(value);
    };
    let snapshot = object.snapshot()?;
    let cases = snapshot
        .as_any()
        .downcast_ref::<Collection<EnumCase>>()
        .or_else(|| {
            snapshot
                .as_any()
                .downcast_ref::<EnumArrayObject>()
                .map(EnumArrayObject::as_collection)
        });
    if let Some(cases) = cases {
        Ok(ModelObject::new(EnumCaseArray(cases.clone())).into())
    } else {
        value.into_model_array_value().map(AttributeValue::from)
    }
}

/// 枚举数组对象转换。
pub struct AsEnumArrayObject;

impl AsEnumArrayObject {
    /// 使用声明的枚举解析元素。保存时按遍历顺序重编号。
    pub fn of(cast: EnumCast) -> AttributeCast {
        EnumCollectionCast {
            cast,
            array_object: true,
        }
        .declaration()
    }
}

/// 枚举集合转换。
pub struct AsEnumCollection;

impl AsEnumCollection {
    /// 使用声明的枚举解析元素。读取、保存和数组投影保留键。
    pub fn of(cast: EnumCast) -> AttributeCast {
        EnumCollectionCast {
            cast,
            array_object: false,
        }
        .declaration()
    }
}

#[derive(Clone)]
struct EnumCollectionCast {
    cast: EnumCast,
    array_object: bool,
}

impl EnumCollectionCast {
    fn declaration(self) -> AttributeCast {
        AttributeCast::Class(
            ClassCast::new(move || Ok(self.clone()))
                .with_serialization()
                .with_enum_array_projection()
                .with_json_array_comparison(),
        )
    }
}

impl ClassCaster for EnumCollectionCast {
    fn get_value(
        &self,
        name: &str,
        _: Option<&AttributeValue>,
        raw: &AttributeMap,
    ) -> Result<AttributeValue> {
        let Some(raw) = raw.get(name).filter(|value| !value.is_null()) else {
            return Ok(Value::Null.into());
        };
        let Some(values) = Collection::from_stored_array(Json::decode(raw.as_json()?, Some(true))?)
        else {
            return Ok(Value::Null.into());
        };
        let values = values.map_stored(|value, _| self.cast.resolve_case(&value))?;
        Ok(if self.array_object {
            ModelObject::new(EnumArrayObject::new(values)).into()
        } else {
            ModelObject::new(values).into()
        })
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        if value.is_null() {
            return Ok(ClassCastWrite::Value(value));
        }
        let value = value.into_array_json()?;
        let value = match value {
            Value::Object(values) if self.array_object => {
                Value::Array(values.into_values().collect())
            }
            Value::Object(values)
                if values
                    .keys()
                    .enumerate()
                    .all(|(i, key)| *key == i.to_string()) =>
            {
                Value::Array(values.into_values().collect())
            }
            array @ (Value::Array(_) | Value::Object(_)) => array,
            scalar if !self.array_object => Value::Array(vec![scalar]),
            _ => {
                return Err(Error::custom(
                    "model_enum_array_requires_iterable",
                    "枚举数组对象的输入必须可迭代",
                ));
            }
        };
        Ok(ClassCastWrite::Value(Json::encode(&value, 0)?.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::eloquent::{ModelAttributes, ModelMetadata},
        support::BackedEnum,
    };
    use serde_json::{Map, json};

    #[derive(Debug, PartialEq, Eq)]
    enum Flag {
        Off,
        On,
    }

    impl BackedEnum for Flag {
        type Backing = i64;

        fn try_from_backing(value: i64) -> Option<Self> {
            match value {
                0 => Some(Self::Off),
                1 => Some(Self::On),
                _ => None,
            }
        }
    }

    fn flags() -> EnumCast {
        EnumCast::backed(|flag| match flag {
            Flag::Off => 0,
            Flag::On => 1,
        })
    }

    fn model(cast: AttributeCast) -> ModelMetadata {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert("data".into(), cast);
        model
    }

    fn hydrated(raw: Value) -> ModelAttributes {
        ModelAttributes::new(Map::from_iter([("data".into(), raw)]))
    }

    fn missing() -> Error {
        Error::custom("test_enum_collection_missing", "缺少模型枚举集合")
    }

    fn object(attrs: &ModelAttributes, metadata: &ModelMetadata) -> Result<ModelObject> {
        attrs
            .get_attribute_value(metadata, "data")?
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(missing)
    }

    #[test]
    fn sparse_enum_collections_preserve_read_keys_and_distinguish_cache_writeback() -> Result<()> {
        for (cast, array_object) in [
            (AsEnumArrayObject::of(flags()), true),
            (AsEnumCollection::of(flags()), false),
        ] {
            let metadata = model(cast);
            let attrs = hydrated(json!(r#"{"2":1,"x":0}"#));
            let value = object(&attrs, &metadata)?;
            assert!(value.same_object(&object(&attrs, &metadata)?));
            let snapshot = value.snapshot()?;
            let entries = if array_object {
                snapshot
                    .as_any()
                    .downcast_ref::<EnumArrayObject>()
                    .ok_or_else(missing)?
                    .as_collection()
            } else {
                snapshot
                    .as_any()
                    .downcast_ref::<Collection<EnumCase>>()
                    .ok_or_else(missing)?
            };
            assert_eq!(
                entries.get(2).and_then(EnumCase::downcast_ref::<Flag>),
                Some(&Flag::On)
            );
            assert_eq!(
                entries.get("x").and_then(EnumCase::downcast_ref::<Flag>),
                Some(&Flag::Off)
            );
            assert_eq!(
                attrs.attributes_to_json(&metadata)?["data"],
                json!({"2":1,"x":0})
            );
            assert_eq!(attrs.is_dirty(&metadata, &["data"])?, array_object);
            let expected = if array_object {
                "[1,0]"
            } else {
                r#"{"2":1,"x":0}"#
            };
            assert_eq!(attrs.raw_attribute("data")?, Some(json!(expected)));
        }
        Ok(())
    }

    #[tokio::test]
    async fn enum_collection_writes_do_not_validate_target_cases_and_keep_null_distinct()
    -> Result<()> {
        let cast = flags();
        for (declaration, array_object) in [
            (AsEnumArrayObject::of(cast.clone()), true),
            (AsEnumCollection::of(cast.clone()), false),
        ] {
            let metadata = model(declaration);
            let attrs = ModelAttributes::default();
            attrs
                .set_attribute(
                    &metadata,
                    "data",
                    json!({"x":77,"null":null,"other":{"raw":true}}),
                )
                .await?;
            let expected = if array_object {
                r#"[77,null,{"raw":true}]"#
            } else {
                r#"{"x":77,"null":null,"other":{"raw":true}}"#
            };
            assert_eq!(attrs.raw_attribute("data")?, Some(json!(expected)));
            assert!(attrs.get_attribute_value(&metadata, "data").is_err());
            attrs.set_attribute(&metadata, "data", Value::Null).await?;
            assert_eq!(attrs.raw_attribute("data")?, Some(Value::Null));
            assert!(
                attrs
                    .get_attribute_value(&metadata, "data")?
                    .is_some_and(|value| value.is_null())
            );
            let scalar = attrs.set_attribute(&metadata, "data", json!(true)).await;
            if array_object {
                assert!(
                    matches!(scalar, Err(error) if error.code() == "model_enum_array_requires_iterable")
                );
            } else {
                scalar?;
                assert_eq!(attrs.raw_attribute("data")?, Some(json!("[true]")));
            }
        }
        let metadata = model(AsEnumCollection::of(cast));
        let attrs = ModelAttributes::default();
        attrs
            .set_attribute(&metadata, "data", json!({"0":1,"1":0}))
            .await?;
        assert_eq!(attrs.raw_attribute("data")?, Some(json!("[1,0]")));
        Ok(())
    }

    #[test]
    fn enum_collection_roots_and_elements_have_separate_decode_rules() -> Result<()> {
        for cast in [
            AsEnumArrayObject::of(flags()),
            AsEnumCollection::of(flags()),
        ] {
            let metadata = model(cast);
            for raw in [
                Value::Null,
                json!("bad"),
                json!("null"),
                json!("1"),
                json!(r#""text""#),
            ] {
                assert!(
                    hydrated(raw)
                        .get_attribute_value(&metadata, "data")?
                        .is_some_and(|value| value.is_null())
                );
            }
            let empty = hydrated(json!("[]"));
            assert!(object(&empty, &metadata).is_ok());
            let attrs = hydrated(json!("[null]"));
            assert_eq!(attrs.attributes_to_json(&metadata)?["data"], json!([0]));
            let attrs = hydrated(json!("[99]"));
            assert!(attrs.is_clean(&metadata, &["data"])?);
            assert!(attrs.get_attribute_value(&metadata, "data").is_err());
        }
        Ok(())
    }

    #[test]
    fn same_name_append_preserves_cases_without_running_class_serialization() -> Result<()> {
        use crate::database::eloquent::ModelAttribute;
        for array_object in [true, false] {
            let unit = EnumCast::unit(|name| (name == "On").then_some(Flag::On), |_: &Flag| "On");
            for (cast, raw, backed) in [
                (unit, r#"{"2":"On"}"#, false),
                (flags(), r#"{"2":1}"#, true),
            ] {
                let mut metadata = model(if array_object {
                    AsEnumArrayObject::of(cast)
                } else {
                    AsEnumCollection::of(cast)
                });
                let attrs = hydrated(json!(raw));
                let expected: Value = serde_json::from_str(raw).map_err(Error::internal)?;
                assert_eq!(attrs.attributes_to_json(&metadata)?["data"], expected);
                metadata.append("data");
                let array = attrs.attributes_to_array(&metadata)?;
                let snapshot = array
                    .get("data")
                    .and_then(AttributeValue::as_object)
                    .ok_or_else(missing)?
                    .snapshot()?;
                let cases = snapshot
                    .as_any()
                    .downcast_ref::<Collection<EnumCase>>()
                    .ok_or_else(missing)?;
                assert_eq!(
                    cases.get(2).and_then(EnumCase::downcast_ref::<Flag>),
                    Some(&Flag::On)
                );
                if backed {
                    assert_eq!(attrs.attributes_to_json(&metadata)?["data"], expected);
                } else {
                    assert!(attrs.attributes_to_json(&metadata).is_err());
                }
                metadata.without_appends();
                attrs.register_attribute(
                    "data",
                    ModelAttribute::new().get(|_, _| Ok(json!("getter").into())),
                )?;
                assert_eq!(
                    attrs.get_attribute(&metadata, "data")?,
                    Some(json!("getter"))
                );
                let output = attrs.attributes_to_array(&metadata)?;
                assert!(
                    output
                        .get("data")
                        .and_then(AttributeValue::as_object)
                        .is_some()
                );
                assert_eq!(attrs.attributes_to_json(&metadata).is_ok(), backed);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn enum_collection_accepts_foreign_cases_for_storage_and_rejects_them_on_read()
    -> Result<()> {
        enum Foreign {
            Other,
        }
        let foreign = EnumCast::unit(|_| Some(Foreign::Other), |_: &Foreign| "Other");
        let value = Collection::try_from_entries([(
            crate::support::ArrayKey::from("foreign"),
            foreign.case(Foreign::Other)?,
        )])?;
        let metadata = model(AsEnumCollection::of(flags()));
        let attrs = ModelAttributes::default();
        attrs
            .set_attribute_value(&metadata, "data", ModelObject::new(value).into())
            .await?;
        assert_eq!(
            attrs.raw_attribute("data")?,
            Some(json!(r#"{"foreign":"Other"}"#))
        );
        attrs.set_raw_attribute("data", json!(r#"{"foreign":"Other"}"#))?;
        assert!(attrs.get_attribute_value(&metadata, "data").is_err());
        Ok(())
    }
}
