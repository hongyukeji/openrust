//! JSON 数组对象和集合转换。
use super::{
    AttributeCast, AttributeMap, AttributeValue, ClassCast, ClassCastWrite, ClassCaster, Json,
    ModelObject, attribute_cast::encode_array_json,
};
use crate::{
    Result,
    support::{ArrayKey, Collection, DataObject},
};
use serde::Serialize;
use serde_json::Value;
use std::{any::Any, sync::Arc};

#[cfg(feature = "encryption")]
#[path = "encrypted_collection_cast.rs"]
mod encrypted;
#[cfg(feature = "encryption")]
pub use encrypted::{AsEncryptedArrayObject, AsEncryptedCollection};

/// Eloquent 的可变数组对象。模型句柄中的修改通过 `ModelObject::modify` 发布。
#[derive(Clone)]
pub struct ArrayObject(Collection);

impl ArrayObject {
    /// 由保持 PHP 键顺序的集合创建数组对象。
    pub const fn new(values: Collection) -> Self {
        Self(values)
    }

    /// 返回数组的底层集合。
    pub const fn as_collection(&self) -> &Collection {
        &self.0
    }

    /// 在独立快照上修改数组内容。
    pub const fn as_collection_mut(&mut self) -> &mut Collection {
        &mut self.0
    }

    /// 复制成 PHP 数组形状对应的 JSON 值。
    pub fn array_copy(&self) -> Result<Value> {
        serde_json::to_value(&self.0).map_err(crate::Error::internal)
    }
}

impl DataObject for ArrayObject {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn json_projection(&self) -> Option<Value> {
        self.array_copy().ok()
    }
}

impl<T: Serialize + Send + Sync + 'static> DataObject for Collection<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn json_projection(&self) -> Option<Value> {
        serde_json::to_value(self).ok()
    }
}

/// 自定义集合包装的类型契约。
pub trait CollectionCastValue: DataObject {
    /// 集合元素类型。
    type Item;

    /// 返回底层有序集合。
    fn as_collection(&self) -> &Collection<Self::Item>;
}

impl<T: Serialize + Send + Sync + 'static> CollectionCastValue for Collection<T> {
    type Item = T;

    fn as_collection(&self) -> &Collection<T> {
        self
    }
}

/// 转换为可写回模型的数组对象。
pub struct AsArrayObject;

impl AsArrayObject {
    /// 默认将赋值 null 编码为 JSON 文本。
    pub fn cast() -> AttributeCast {
        array_object(false)
    }

    /// 将赋值 null 保存为 SQL NULL。
    pub fn nullable() -> AttributeCast {
        array_object(true)
    }
}

fn array_object(nullable: bool) -> AttributeCast {
    let factory = CollectionFactory::new(ArrayObject::new);
    let cast = ClassCast::new(move || {
        Ok(JsonCollectionCast {
            factory: factory.clone(),
            nullable,
        })
    })
    .with_serialization()
    .with_json_array_comparison()
    .with_arrayable_projection();
    AttributeCast::Class(cast)
}

type Factory = dyn Fn(Collection) -> Result<ModelObject> + Send + Sync;

#[derive(Clone)]
struct CollectionFactory(Arc<Factory>);

impl CollectionFactory {
    fn new<T: DataObject>(build: impl Fn(Collection) -> T + Send + Sync + 'static) -> Self {
        Self::fallible(move |values| Ok(build(values)))
    }

    fn fallible<T: DataObject>(
        build: impl Fn(Collection) -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(move |values| build(values).map(ModelObject::new)))
    }
}

/// 保留已映射元素类型和键的集合。
pub struct ObjectCollection<T>(Collection<T>);

impl<T> ObjectCollection<T> {
    /// 借用有类型的集合元素。
    pub const fn as_collection(&self) -> &Collection<T> {
        &self.0
    }

    /// 按现有键变换元素，适用于通过模型对象句柄替换快照。
    pub fn map_items<U>(
        &self,
        map: impl FnMut(&T, &ArrayKey) -> Result<U>,
    ) -> Result<ObjectCollection<U>> {
        self.0.project_stored(map).map(ObjectCollection)
    }
}

impl<T: DataObject> DataObject for ObjectCollection<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn json_projection(&self) -> Option<Value> {
        self.project(DataObject::json_projection)
    }

    fn array_projection(&self) -> Option<Value> {
        self.project(DataObject::array_projection)
    }
}

impl<T: DataObject> CollectionCastValue for ObjectCollection<T> {
    type Item = T;

    fn as_collection(&self) -> &Collection<T> {
        &self.0
    }
}

impl<T: DataObject> ObjectCollection<T> {
    fn project(&self, project: impl Fn(&T) -> Option<Value>) -> Option<Value> {
        let values = self
            .0
            .project_stored(|value, _| {
                project(value).ok_or_else(super::attribute_value::object_projection_required)
            })
            .ok()?;
        serde_json::to_value(values).ok()
    }
}

/// 集合转换声明，可配置具体集合类型和键感知的元素映射。
#[derive(Clone)]
pub struct AsCollection {
    factory: CollectionFactory,
    nullable: bool,
}

impl Default for AsCollection {
    fn default() -> Self {
        Self::using(Ok)
    }
}

impl AsCollection {
    /// 使用类型安全的集合工厂，可在工厂中构造应用自己的集合包装。
    pub fn using<T: CollectionCastValue>(
        factory: impl Fn(Collection) -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            factory: CollectionFactory::fallible(factory),
            nullable: false,
        }
    }

    /// 映射每个值和键，保留原键，并输出保存映射值的集合。
    pub fn of<T: Serialize + Send + Sync + 'static>(
        map: impl Fn(Value, &ArrayKey) -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        Self::using(move |collection| collection.map_stored(&map))
    }

    /// 映射为有类型对象，分别应用元素的数组及 JSON 投影。
    pub fn of_objects<T: DataObject>(
        map: impl Fn(Value, &ArrayKey) -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        Self::using(move |collection| collection.map_stored(&map).map(ObjectCollection))
    }

    /// 将赋值 null 保存为 SQL NULL。
    #[must_use]
    pub const fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// 创建可放入模型 casts 声明的类转换。
    pub fn cast(self) -> AttributeCast {
        AttributeCast::Class(
            ClassCast::new(move || {
                Ok(JsonCollectionCast {
                    factory: self.factory.clone(),
                    nullable: self.nullable,
                })
            })
            .with_json_array_comparison()
            .with_arrayable_projection(),
        )
    }
}

struct JsonCollectionCast {
    factory: CollectionFactory,
    nullable: bool,
}

impl ClassCaster for JsonCollectionCast {
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
        (self.factory.0)(values).map(AttributeValue::from)
    }

    fn set(&self, _: &str, value: AttributeValue, _: &AttributeMap) -> Result<ClassCastWrite> {
        if value.is_null() && self.nullable {
            return Ok(ClassCastWrite::Value(value));
        }
        let value = value.into_array_json()?;
        Ok(ClassCastWrite::Value(encode_array_json(&value)?.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::eloquent::{ModelAttributes, ModelMetadata};
    use serde_json::{Map, json};

    fn metadata(cast: AttributeCast) -> ModelMetadata {
        let mut model = ModelMetadata::new("records", "Record");
        model.casts.insert("data".into(), cast);
        model
    }

    fn object(store: &ModelAttributes, model: &ModelMetadata) -> Result<ModelObject> {
        store
            .get_attribute_value(model, "data")?
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(|| crate::Error::custom("test_collection_cast_object", "缺少已转换对象"))
    }

    #[tokio::test]
    async fn array_object_preserves_keys_and_writes_nullable_or_json_null() -> Result<()> {
        let model = metadata(AsArrayObject::cast());
        let store = ModelAttributes::new(Map::from_iter([(
            "data".into(),
            json!(r#"{"0":{"0":"a"},"2":{},"tag":"old"}"#),
        )]));
        let value = object(&store, &model)?;
        assert!(value.same_object(&object(&store, &model)?));
        assert_eq!(
            store.attributes_to_json(&model)?["data"],
            json!({"0":["a"],"2":[],"tag":"old"})
        );
        value.modify(|current: &ArrayObject| {
            let mut next = current.clone();
            next.as_collection_mut().try_put("tag", json!("new"))?;
            Ok(next)
        })?;
        assert_eq!(
            store.raw_attribute("data")?,
            Some(json!(r#"{"0":["a"],"2":[],"tag":"new"}"#))
        );
        assert_eq!(
            store.attributes_to_array(&model)?["data"].as_json()?["tag"],
            json!("new")
        );

        store.set_attribute(&model, "data", Value::Null).await?;
        assert_eq!(store.raw_attribute("data")?, Some(json!("null")));
        let nullable = metadata(AsArrayObject::nullable());
        store.set_attribute(&nullable, "data", Value::Null).await?;
        assert_eq!(store.raw_attribute("data")?, Some(Value::Null));
        for raw in [
            json!("12"),
            json!("broken"),
            json!("true"),
            json!("null"),
            Value::Null,
        ] {
            store.set_raw_attribute("data", raw)?;
            assert_eq!(store.get_attribute(&model, "data")?, Some(Value::Null));
        }
        Ok(())
    }

    #[test]
    fn collection_maps_values_and_keys_and_compares_raw_decoded_arrays() -> Result<()> {
        let model = metadata(AsCollection::of(|value, key| Ok((key.to_string(), value))).cast());
        let store = ModelAttributes::new(Map::from_iter([(
            "data".into(),
            json!(r#"{"2":"two","tag":"old"}"#),
        )]));
        assert_eq!(
            store.attributes_to_array(&model)?["data"].as_json()?,
            &json!({"2":["2","two"],"tag":["tag","old"]})
        );
        let plain = metadata(AsCollection::default().cast());
        let store = ModelAttributes::new(Map::from_iter([(
            "data".into(),
            json!(r#"{"0":1,"1":{}}"#),
        )]));
        store.set_raw_attribute("data", json!("[1, []]"))?;
        assert!(store.original_is_equivalent(&plain, "data")?);
        store.set_raw_attribute("data", json!("[1.0, []]"))?;
        assert!(!store.original_is_equivalent(&plain, "data")?);
        Ok(())
    }

    struct DualProjection;

    impl DataObject for DualProjection {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn json_projection(&self) -> Option<Value> {
            Some(json!({"json": 2}))
        }

        fn array_projection(&self) -> Option<Value> {
            Some(json!({"array": 1}))
        }
    }

    #[test]
    fn mapped_collection_separates_array_projection_and_storage_json() -> Result<()> {
        let model = metadata(AsCollection::of_objects(|_, _| Ok(DualProjection)).cast());
        let store = ModelAttributes::new(Map::from_iter([("data".into(), json!("[0]"))]));
        assert_eq!(
            store.attributes_to_array(&model)?["data"].as_json()?,
            &json!([{"array":1}])
        );
        assert_eq!(store.raw_attribute("data")?, Some(json!(r#"[{"json":2}]"#)));
        Ok(())
    }
}
