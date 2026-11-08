//! 原生模型枚举转换。枚举实例以不可变类型句柄保存，原始列仍是 backing 或 case 名。
use super::{AttributeValue, attribute_cast::scalar_string};
use crate::{
    Error, Result,
    support::{BackedEnum, EnumBacking},
};
use serde_json::Value;
use std::{any::Any, fmt, sync::Arc};

type Case = dyn Any + Send + Sync;
type Resolve = dyn Fn(&Value) -> Result<Arc<Case>> + Send + Sync;
type Project = dyn Fn(&Case) -> Result<Value> + Send + Sync;

/// 模型读出的不可变枚举 case。
#[derive(Clone)]
pub struct EnumCase {
    value: Arc<Case>,
    storable: Value,
}

impl EnumCase {
    /// 借用应用声明的具体 Rust 枚举。
    pub fn downcast_ref<E: Send + Sync + 'static>(&self) -> Option<&E> {
        self.value.downcast_ref()
    }

    /// 返回用于持久化和数组输出的 backing 或 case 名。
    pub const fn storable(&self) -> &Value {
        &self.storable
    }

    pub(super) fn into_storable(self) -> Value {
        self.storable
    }

    pub(super) fn same_case(&self, other: &Self) -> bool {
        self.value.as_ref().type_id() == other.value.as_ref().type_id()
            && self.storable == other.storable
    }
}

impl fmt::Debug for EnumCase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnumCase([REDACTED])")
    }
}

/// Eloquent 的原生枚举声明，对应 Laravel 的 `UnitEnum` 和 `BackedEnum`。
#[derive(Clone)]
pub struct EnumCast(Arc<Definition>);

struct Definition {
    resolve: Box<Resolve>,
    project: Box<Project>,
}

impl EnumCast {
    /// 复用应用的 backing 查找规则，提供 case 到 backing 的反向映射。
    pub fn backed<E>(backing: fn(&E) -> E::Backing) -> Self
    where
        E: BackedEnum + Send + Sync + 'static,
        E::Backing: Into<Value>,
    {
        Self(Arc::new(Definition {
            resolve: Box::new(|raw| {
                let backing = E::Backing::from_input(raw).map_err(|_| invalid_value())?;
                E::try_from_backing(backing)
                    .map(|case| Arc::new(case) as Arc<Case>)
                    .ok_or_else(invalid_value)
            }),
            project: Box::new(move |value| {
                value
                    .downcast_ref::<E>()
                    .map(|case| backing(case).into())
                    .ok_or_else(wrong_type)
            }),
        }))
    }

    /// 通过 case 名查找应用枚举，并为数组和持久化输出其规范名称。
    pub fn unit<E>(from_name: fn(&str) -> Option<E>, name: fn(&E) -> &'static str) -> Self
    where
        E: Send + Sync + 'static,
    {
        Self(Arc::new(Definition {
            resolve: Box::new(move |raw| {
                let name = scalar_string(raw).map_err(|_| invalid_value())?;
                from_name(&name)
                    .map(|case| Arc::new(case) as Arc<Case>)
                    .ok_or_else(invalid_value)
            }),
            project: Box::new(move |value| {
                value
                    .downcast_ref::<E>()
                    .map(|case| Value::from(name(case)))
                    .ok_or_else(wrong_type)
            }),
        }))
    }

    /// 将应用构造的枚举传给模型的有类型属性赋值接口。
    pub fn case<E: Send + Sync + 'static>(&self, value: E) -> Result<EnumCase> {
        let storable = (self.0.project)(&value)?;
        Ok(EnumCase {
            value: Arc::new(value),
            storable,
        })
    }

    pub(super) fn read(&self, raw: AttributeValue) -> Result<AttributeValue> {
        if raw.is_null() {
            return Ok(raw);
        }
        if let AttributeValue::Enum(case) = &raw {
            (self.0.project)(&*case.value)?;
            return Ok(raw);
        }
        let value = (self.0.resolve)(raw.as_json().map_err(|_| wrong_type())?)?;
        let storable = (self.0.project)(&*value)?;
        Ok(EnumCase { value, storable }.into())
    }

    pub(super) fn write(&self, value: AttributeValue) -> Result<AttributeValue> {
        if let AttributeValue::Enum(case) = &value {
            return (self.0.project)(&*case.value).map(AttributeValue::from);
        }
        self.read(value)?.try_into_json().map(AttributeValue::from)
    }
}

impl fmt::Debug for EnumCast {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnumCast")
    }
}

impl PartialEq for EnumCast {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for EnumCast {}

fn invalid_value() -> Error {
    Error::custom(
        "model_enum_value_invalid",
        "模型属性值无法解析为声明的枚举 case",
    )
}

fn wrong_type() -> Error {
    Error::custom("model_enum_type_mismatch", "模型属性的枚举类型与声明不匹配")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata};
    use serde_json::{Map, json};

    #[derive(Debug, PartialEq, Eq)]
    enum State {
        Open,
        Closed,
    }

    impl BackedEnum for State {
        type Backing = i64;

        fn try_from_backing(value: i64) -> Option<Self> {
            match value {
                0 => Some(Self::Open),
                1 => Some(Self::Closed),
                _ => None,
            }
        }
    }

    fn numeric_backing(value: &State) -> i64 {
        match value {
            State::Open => 0,
            State::Closed => 1,
        }
    }

    fn state_name(value: &State) -> &'static str {
        match value {
            State::Open => "Open",
            State::Closed => "Closed",
        }
    }

    fn state_from_name(value: &str) -> Option<State> {
        match value {
            "Open" | "DefaultState" => Some(State::Open),
            "Closed" => Some(State::Closed),
            _ => None,
        }
    }

    #[test]
    fn hydration_preserves_types_and_projects_enum_backing_or_unit_name() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "Record");
        metadata.casts.insert(
            "number".into(),
            AttributeCast::Enum(EnumCast::backed(numeric_backing)),
        );
        metadata.casts.insert(
            "unit".into(),
            AttributeCast::Enum(EnumCast::unit(state_from_name, state_name)),
        );
        let attrs = ModelAttributes::new(Map::from_iter([
            ("number".into(), json!("01")),
            ("unit".into(), json!("DefaultState")),
        ]));
        let value = attrs
            .get_attribute_value(&metadata, "number")?
            .ok_or_else(invalid_value)?;
        assert_eq!(
            value.as_enum().and_then(EnumCase::downcast_ref::<State>),
            Some(&State::Closed)
        );
        let result = attrs.attributes_to_json(&metadata)?;
        assert_eq!(result.get("number"), Some(&json!(1)));
        assert_eq!(result.get("unit"), Some(&json!("Open")));
        assert_eq!(attrs.get_raw_original("number")?, Some(json!("01")));
        assert_eq!(attrs.get_attribute(&metadata, "number")?, Some(json!(1)));
        assert!(attrs.get_attribute_value(&metadata, "missing")?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn typed_enum_writes_validate_type_and_keep_full_key_dispatch() -> Result<()> {
        enum Foreign {
            Closed,
        }
        let cast = EnumCast::backed(numeric_backing);
        let foreign = EnumCast::unit(|_| Some(Foreign::Closed), |_: &Foreign| "Closed");
        let mut metadata = ModelMetadata::new("records", "Record");
        metadata
            .casts
            .insert("bag->state".into(), AttributeCast::Enum(cast.clone()));
        metadata.casts.insert("bag".into(), AttributeCast::Array);
        let attrs = ModelAttributes::new(Map::from_iter([(
            "bag".into(),
            json!(r#"{"before":true}"#),
        )]));
        attrs
            .set_attribute_value(&metadata, "bag->state", cast.case(State::Closed)?.into())
            .await?;
        attrs
            .set_attribute(&metadata, "bag->other", json!(true))
            .await?;
        assert_eq!(attrs.raw_attribute("bag->state")?, Some(json!(1)));
        assert_eq!(
            attrs.get_attribute(&metadata, "bag")?,
            Some(json!({"before":true,"other":true}))
        );
        let foreign_value = foreign.case(Foreign::Closed)?;
        let result = attrs
            .set_attribute_value(&metadata, "bag->state", foreign_value.into())
            .await;
        assert!(matches!(result, Err(error) if error.code() == "model_enum_type_mismatch"));
        for bad in [json!("1a"), json!({}), json!([1]), json!(9)] {
            assert!(
                attrs
                    .set_attribute(&metadata, "bag->state", bad)
                    .await
                    .is_err()
            );
            assert_eq!(attrs.raw_attribute("bag->state")?, Some(json!(1)));
        }
        attrs
            .set_attribute(&metadata, "bag->state", Value::Null)
            .await?;
        assert_eq!(attrs.raw_attribute("bag->state")?, Some(Value::Null));
        Ok(())
    }

    #[tokio::test]
    async fn enum_assignment_does_not_flush_an_unrelated_object_cache() -> Result<()> {
        use crate::{
            database::eloquent::{
                ArrayObject, AttributeMap, ClassCast, ClassCastWrite, ClassCaster, ModelAttribute,
                ModelObject,
            },
            support::Collection,
        };

        struct FailingWrite;
        impl ClassCaster for FailingWrite {
            fn get_value(
                &self,
                _: &str,
                _: Option<&AttributeValue>,
                _: &AttributeMap,
            ) -> Result<AttributeValue> {
                Ok(ModelObject::new(ArrayObject::new(Collection::from_value(json!([]))?)).into())
            }

            fn set(&self, _: &str, _: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
                Err(Error::custom(
                    "test_enum_unrelated_cache",
                    "不相关的回写被执行",
                ))
            }
        }

        let mut metadata = ModelMetadata::new("records", "Record");
        metadata.casts.insert(
            "state".into(),
            AttributeCast::Enum(EnumCast::backed(numeric_backing)),
        );
        metadata.casts.insert(
            "other".into(),
            AttributeCast::Class(ClassCast::new(|| Ok(FailingWrite))),
        );
        let attrs = ModelAttributes::default();
        assert!(attrs.get_attribute_value(&metadata, "other")?.is_some());
        attrs.set_attribute(&metadata, "state", json!(1)).await?;
        assert_eq!(attrs.raw_attribute("state")?, Some(json!(1)));
        assert!(
            matches!(attrs.raw_attribute("other"), Err(error) if error.code() == "test_enum_unrelated_cache")
        );
        attrs.register_attribute(
            "state",
            ModelAttribute::new()
                .set(|value, _| Ok(AttributeMap::from_iter([("state".into(), value)]))),
        )?;
        let result = attrs
            .set_attribute(&metadata, "state", json!("from custom setter"))
            .await;
        assert!(matches!(result, Err(error) if error.code() == "test_enum_unrelated_cache"));
        assert_eq!(attrs.raw_attribute("state")?, Some(json!(1)));
        attrs.set_raw_attribute("other", Value::Null)?;
        attrs
            .set_attribute(&metadata, "state", json!("from custom setter"))
            .await?;
        assert_eq!(
            attrs.raw_attribute("state")?,
            Some(json!("from custom setter"))
        );
        Ok(())
    }

    #[test]
    fn dirty_compares_raw_values_without_resolving_enum_cases() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "Record");
        metadata.casts.insert(
            "state".into(),
            AttributeCast::Enum(EnumCast::backed(numeric_backing)),
        );
        let attrs = ModelAttributes::new(Map::from_iter([("state".into(), json!(1))]));
        for (raw, equivalent) in [
            (json!("1"), true),
            (json!(1.0), true),
            (json!("01"), false),
            (json!("1e0"), false),
            (json!(true), false),
        ] {
            attrs.set_raw_attribute("state", raw)?;
            assert_eq!(
                attrs.original_is_equivalent(&metadata, "state")?,
                equivalent
            );
        }
        attrs.set_raw_attribute("state", json!("unknown"))?;
        attrs.sync_original()?;
        assert!(attrs.is_clean(&metadata, &["state"])?);
        assert!(attrs.get_attribute(&metadata, "state").is_err());
        Ok(())
    }

    #[test]
    fn backed_conversion_reuses_php_weak_numeric_and_string_rules() -> Result<()> {
        enum Text {
            Whole,
            Fraction,
        }
        impl BackedEnum for Text {
            type Backing = String;
            fn try_from_backing(value: String) -> Option<Self> {
                match value.as_str() {
                    "1" => Some(Self::Whole),
                    "1.5" => Some(Self::Fraction),
                    _ => None,
                }
            }
        }
        let text = EnumCast::backed(|value| match value {
            Text::Whole => "1".into(),
            Text::Fraction => "1.5".into(),
        });
        for (input, stored) in [
            (json!(true), json!("1")),
            (json!(1.5), json!("1")),
            (json!("1.5"), json!("1.5")),
        ] {
            assert_eq!(text.write(input.into())?.try_into_json()?, stored);
        }
        let numeric = EnumCast::backed(numeric_backing);
        for input in [
            json!(true),
            json!(1.5),
            json!("01"),
            json!("+1"),
            json!(" 1e0 "),
        ] {
            assert_eq!(numeric.write(input.into())?.try_into_json()?, json!(1));
        }
        Ok(())
    }
}
