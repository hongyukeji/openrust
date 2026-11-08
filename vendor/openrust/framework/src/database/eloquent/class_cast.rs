//! 显式注册的 Eloquent 自定义类转换。
use super::{AttributeMap, AttributeValue};
use crate::Result;
use serde_json::Value;
use std::{fmt, sync::Arc};

/// 自定义类转换，读取与比较使用模型原始值，写入返回原始列补丁。
///
/// 默认读取透传，适用于仅入站转换。当前读取输出为 JSON 值，数组和映射按值返回。
pub trait ClassCaster {
    /// 读取已水合的原始值；缺少字段时传入 None。
    fn get(
        &self,
        _name: &str,
        value: Option<&AttributeValue>,
        _raw: &AttributeMap,
    ) -> Result<Value> {
        value
            .cloned()
            .map_or(Ok(Value::Null), AttributeValue::try_into_json)
    }

    /// 将输入转换为一个或多个原始列值。
    fn set(&self, name: &str, value: AttributeValue, raw: &AttributeMap) -> Result<ClassCastWrite>;

    /// 投影非 null 的转换值。默认保留读取结果。
    fn serialize(&self, _name: &str, value: Value, _raw: &AttributeMap) -> Result<Value> {
        Ok(value)
    }

    /// 比较原始基线与当前值。None 使用模型的默认等价规则。
    fn compare(
        &self,
        _name: &str,
        _original: &AttributeValue,
        _current: &AttributeValue,
    ) -> Result<Option<bool>> {
        Ok(None)
    }
}

/// 类转换的单列值或多列写入补丁。
pub enum ClassCastWrite {
    /// 写入声明转换的字段。
    Value(AttributeValue),
    /// 依序替换列值。
    Patch(AttributeMap),
}

impl ClassCastWrite {
    pub(super) fn into_patch(self, name: &str) -> AttributeMap {
        match self {
            Self::Value(value) => AttributeMap::from_iter([(name.to_owned(), value)]),
            Self::Patch(values) => values,
        }
    }
}

type Resolver = dyn Fn() -> Result<Box<dyn ClassCaster>> + Send + Sync;

/// 模型声明的自定义转换工厂。声明克隆共享同一个工厂。
#[derive(Clone)]
pub struct ClassCast {
    resolver: Arc<Resolver>,
    serialization: bool,
    comparison: bool,
}

impl ClassCast {
    /// 登记构造函数及其捕获的参数。每次需要转换时重新解析转换器。
    pub fn new<T: ClassCaster + 'static>(
        resolve: impl Fn() -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            resolver: Arc::new(move || Ok(Box::new(resolve()?))),
            serialization: false,
            comparison: false,
        }
    }

    /// 启用转换器的数组序列化方法。
    #[must_use]
    pub const fn with_serialization(mut self) -> Self {
        self.serialization = true;
        self
    }

    /// 启用转换器的原值比较方法。
    #[must_use]
    pub const fn with_comparison(mut self) -> Self {
        self.comparison = true;
        self
    }

    pub(super) fn resolve(&self) -> Result<Box<dyn ClassCaster>> {
        (self.resolver)()
    }

    pub(super) fn serialize(&self, name: &str, value: Value, raw: &AttributeMap) -> Result<Value> {
        if value.is_null() {
            return Ok(value);
        }
        // Laravel 分别解析能力检查和实际方法调用。
        drop(self.resolve()?);
        if self.serialization {
            self.resolve()?.serialize(name, value, raw)
        } else {
            Ok(value)
        }
    }

    pub(super) fn compare(
        &self,
        name: &str,
        original: &AttributeValue,
        current: &AttributeValue,
    ) -> Result<Option<bool>> {
        drop(self.resolve()?);
        if self.comparison {
            self.resolve()?.compare(name, original, current)
        } else {
            Ok(None)
        }
    }
}

impl fmt::Debug for ClassCast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClassCast")
    }
}

impl PartialEq for ClassCast {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.resolver, &other.resolver)
            && self.serialization == other.serialization
            && self.comparison == other.comparison
    }
}

impl Eq for ClassCast {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{
        AttributeCast, ModelAttribute, ModelAttributes, ModelMetadata,
    };
    use serde_json::{Map, json};

    struct Scaled(i64);

    fn number(value: &AttributeValue) -> Result<i64> {
        Ok(crate::support::scalars::integer(value.as_json()?))
    }

    impl ClassCaster for Scaled {
        fn get(
            &self,
            _: &str,
            value: Option<&AttributeValue>,
            raw: &AttributeMap,
        ) -> Result<Value> {
            let units = value.map(number).transpose()?;
            let fallback = raw
                .get("fallback")
                .map(number)
                .transpose()?
                .unwrap_or_default();
            Ok(json!(units.unwrap_or(fallback) * self.0))
        }

        fn set(
            &self,
            name: &str,
            value: AttributeValue,
            _: &AttributeMap,
        ) -> Result<ClassCastWrite> {
            let value = if value.is_null() {
                Value::Null
            } else {
                json!(number(&value)? / self.0)
            };
            Ok(ClassCastWrite::Patch(AttributeMap::from_iter([
                (name.to_owned(), value.into()),
                ("touched".into(), json!(true).into()),
            ])))
        }

        fn serialize(&self, _: &str, value: Value, _: &AttributeMap) -> Result<Value> {
            Ok(json!({"scaled": value}))
        }

        fn compare(
            &self,
            _: &str,
            original: &AttributeValue,
            current: &AttributeValue,
        ) -> Result<Option<bool>> {
            Ok(Some(number(original)? == number(current)?))
        }
    }

    struct Inbound;

    impl ClassCaster for Inbound {
        fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
            Ok(ClassCastWrite::Value(json!(number(&value)? + 1).into()))
        }
    }

    #[tokio::test]
    async fn class_casts_share_assignment_original_dirty_and_projection_paths() -> Result<()> {
        let mut model = ModelMetadata::new("records", "Record");
        let caster = AttributeCast::Class(
            ClassCast::new(|| Ok(Scaled(10)))
                .with_serialization()
                .with_comparison(),
        );
        model.casts.insert("amount".into(), caster.clone());
        model.casts.insert("computed".into(), caster);
        model.casts.insert(
            "inbound".into(),
            AttributeCast::Class(ClassCast::new(|| Ok(Inbound))),
        );
        let attributes = ModelAttributes::new(Map::from_iter([
            ("amount".into(), json!(2)),
            ("fallback".into(), json!(3)),
        ]));

        assert_eq!(attributes.get_attribute(&model, "amount")?, Some(json!(20)));
        assert_eq!(
            attributes.get_attribute(&model, "computed")?,
            Some(json!(30))
        );
        assert_eq!(
            attributes.attributes_to_json(&model)?["amount"],
            json!({"scaled": 20})
        );
        model.append("computed");
        assert_eq!(
            attributes.attributes_to_json(&model)?["computed"],
            json!(30)
        );
        attributes.set_raw_attribute("amount", json!("02"))?;
        assert!(attributes.original_is_equivalent(&model, "amount")?);
        attributes
            .set_attribute(&model, "amount", json!(70))
            .await?;
        assert_eq!(attributes.raw_attribute("amount")?, Some(json!(7)));
        assert_eq!(attributes.raw_attribute("touched")?, Some(json!(true)));
        assert!(attributes.get_dirty(&model)?.contains_key("amount"));
        assert_eq!(
            attributes.get_original(&model, "amount", None, |_| None)?,
            Some(json!(20))
        );

        attributes
            .set_attribute(&model, "inbound", json!(2))
            .await?;
        assert_eq!(attributes.get_attribute(&model, "inbound")?, Some(json!(3)));
        let mut receipt = attributes
            .set_attribute_reversibly_if(&model, "amount", json!(90), |current| {
                Ok(current == Some(json!(70)))
            })
            .await?
            .ok_or_else(|| {
                crate::Error::custom("test_class_cast_receipt_missing", "条件更新未取得收据")
            })?;
        assert_eq!(attributes.raw_attribute("amount")?, Some(json!(9)));
        assert!(receipt.rollback()?);
        assert_eq!(attributes.raw_attribute("amount")?, Some(json!(7)));
        let mut guarded = model.clone();
        guarded.guard(vec!["unrelated".into()]);
        let databases =
            crate::Databases::new(crate::Db::default(), std::collections::BTreeMap::new());
        assert!(
            !attributes
                .is_guarded(&guarded, &databases, "computed")
                .await?
        );

        attributes
            .set_attribute(&model, "amount", Value::Null)
            .await?;
        assert_eq!(attributes.raw_attribute("amount")?, Some(Value::Null));

        attributes.register_attribute(
            "amount",
            ModelAttribute::new().get(|_, _| Ok(json!("getter").into())),
        )?;
        assert_eq!(
            attributes.get_attribute(&model, "amount")?,
            Some(json!("getter"))
        );
        assert_eq!(attributes.attributes_to_json(&model)?["amount"], json!(0));
        Ok(())
    }

    struct Counted {
        ordinal: usize,
        comparisons: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ClassCaster for Counted {
        fn get(&self, name: &str, _: Option<&AttributeValue>, _: &AttributeMap) -> Result<Value> {
            Ok(json!(name))
        }

        fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
            Ok(ClassCastWrite::Value(value))
        }

        fn serialize(&self, _: &str, value: Value, _: &AttributeMap) -> Result<Value> {
            Ok(json!([value, self.ordinal]))
        }

        fn compare(&self, _: &str, _: &AttributeValue, _: &AttributeValue) -> Result<Option<bool>> {
            self.comparisons
                .store(self.ordinal, std::sync::atomic::Ordering::Relaxed);
            Ok(Some(true))
        }
    }

    #[test]
    fn class_cast_array_keeps_field_order_and_resolves_in_declaration_order() -> Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let comparisons = Arc::new(AtomicUsize::new(0));
        let cast = AttributeCast::Class(
            ClassCast::new({
                let comparisons = Arc::clone(&comparisons);
                move || {
                    Ok(Counted {
                        ordinal: count.fetch_add(1, Ordering::Relaxed) + 1,
                        comparisons: Arc::clone(&comparisons),
                    })
                }
            })
            .with_serialization()
            .with_comparison(),
        );
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert("a".into(), cast.clone());
        model.casts.insert("b".into(), cast);
        let attributes = ModelAttributes::new(Map::from_iter([
            ("b".into(), json!(2)),
            ("a".into(), json!(1)),
        ]));

        let projected = attributes.attributes_to_json(&model)?;
        assert_eq!(
            projected.keys().map(String::as_str).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(projected["a"], json!(["a", 3]));
        assert_eq!(projected["b"], json!(["b", 6]));
        attributes.set_raw_attribute("a", json!(3))?;
        assert!(attributes.original_is_equivalent(&model, "a")?);
        assert_eq!(comparisons.load(Ordering::Relaxed), 8);
        Ok(())
    }
}
