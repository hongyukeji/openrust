//! 模型实例已加载的关系值。

use super::{
    AttributeValue, CustomPivot, HasRelationships, HydrateModel, ModelMetadata, Pivot,
    RelationNaming, SaveContext, SaveModel,
};
use crate::{Error, Result, database::TransactionContext, support::ArrayKey};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::{any::Any, cell::RefCell, sync::Arc};

/// 一次批量关系加载及其子路径。
#[derive(Clone, Copy)]
pub struct RelationLoad<'a> {
    /// 当前模型的直接关系名称。
    pub name: &'a str,
    /// 在查得的子模型上继续加载的相对路径。
    pub nested: &'a [&'a str],
}

pub(super) async fn load_requested<M: HydrateModel>(
    models: &mut [M],
    paths: &[&str],
    context: &SaveContext<'_>,
    transaction: Option<&TransactionContext<'_>>,
) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for &path in paths {
        if path.split('.').any(str::is_empty) {
            return Err(Error::custom(
                "model_relation_path_invalid",
                "关系路径含空名称",
            ));
        }
        let (name, nested) = path
            .split_once('.')
            .map_or((path, None), |(name, nested)| (name, Some(nested)));
        let Some((_, children)) = groups.iter_mut().find(|(candidate, _)| *candidate == name)
        else {
            groups.push((name, nested.into_iter().collect()));
            continue;
        };
        if let Some(nested) = nested
            && !children.contains(&nested)
        {
            children.push(nested);
        }
    }
    for (name, nested) in groups {
        M::load_relation(
            models,
            RelationLoad {
                name,
                nested: &nested,
            },
            context,
            transaction,
        )
        .await?;
    }
    Ok(())
}

/// 一个已经加载的关系结果。
pub enum LoadedRelation {
    /// 单值关系已加载，但没有匹配的模型。
    Null,
    /// 已加载的模型或集合，包括空集合。
    Related(Box<dyn Any + Send + Sync>),
    /// 已加载的中间表模型，包括自定义 Pivot。
    Pivot(Box<dyn Any + Send + Sync>),
}

impl LoadedRelation {
    /// 借用已加载的具体值，类型由关系声明决定。
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        match self {
            Self::Null => None,
            Self::Related(value) | Self::Pivot(value) => value.downcast_ref(),
        }
    }
}

/// 模型数组的字段，按属性和已加载关系的投影顺序排列。
pub type ModelArray = IndexMap<String, ModelArrayValue>;

/// 模型数组中的属性、关联模型或关联模型集合。
#[derive(Debug)]
pub enum ModelArrayValue {
    /// 属性值或已加载的空单值关系。
    Attribute(AttributeValue),
    /// 已加载的关联模型。
    Model(ModelArray),
    /// 已加载的关联模型集合。
    Collection(Vec<ModelArray>),
}

fn array_json(values: ModelArray) -> Result<Value> {
    let values: Map<String, Value> = values
        .into_iter()
        .map(|(key, value)| {
            let value = match value {
                ModelArrayValue::Attribute(value) => value.into_array_json()?,
                ModelArrayValue::Model(model) => array_json(model)?,
                ModelArrayValue::Collection(models) => {
                    Value::Array(models.into_iter().map(array_json).collect::<Result<_>>()?)
                }
            };
            Ok((key, value))
        })
        .collect::<Result<_>>()?;
    if values
        .keys()
        .enumerate()
        .all(|(index, key)| *key == index.to_string())
    {
        Ok(Value::Array(values.into_values().collect()))
    } else {
        Ok(Value::Object(values))
    }
}

thread_local! {
    static ARRAY_PATH: RefCell<Vec<*const ()>> = const { RefCell::new(Vec::new()) };
}

struct ArrayVisit;

impl ArrayVisit {
    fn enter(identity: *const ()) -> Option<Self> {
        ARRAY_PATH.with_borrow_mut(|path| {
            if path.contains(&identity) {
                return None;
            }
            path.push(identity);
            Some(Self)
        })
    }
}

impl Drop for ArrayVisit {
    fn drop(&mut self) {
        ARRAY_PATH.with_borrow_mut(|path| {
            path.pop();
        });
    }
}

type ProjectRelation = fn(&LoadedRelation) -> Result<ModelArrayValue>;

struct RelationEntry {
    value: LoadedRelation,
    project: Option<ProjectRelation>,
}

/// 单个模型实例的关系状态，按首次加载顺序保留关系名称。
/// 这里仅保存已加载的结果。读取关系状态不会发起数据库查询。
#[derive(Default)]
pub struct ModelRelations {
    entries: IndexMap<String, RelationEntry>,
}

impl ModelRelations {
    /// 返回已经加载的关系，保留已加载空值与未加载的区别。
    pub fn get_relation(&self, name: &str) -> Option<&LoadedRelation> {
        self.entries.get(name).map(|entry| &entry.value)
    }

    /// 检查关系是否已加载，空值和空集合也视为已加载。
    pub fn relation_loaded(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// 安装或替换已加载结果。替换保留原有顺序。
    /// 非模型的类型擦除值仍可下转。要递归输出模型，使用类型化的关系安装方法。
    pub fn set_relation(&mut self, name: impl Into<String>, value: LoadedRelation) {
        self.insert(name, value, None);
    }

    /// 安装已加载的单值模型；缺少模型时保存 null。
    pub fn set_model<M: HasRelationships>(
        &mut self,
        name: impl Into<String>,
        model: Option<Arc<M>>,
    ) {
        if let Some(model) = model {
            self.insert(
                name,
                LoadedRelation::Related(Box::new(model)),
                Some(project_model::<M>),
            );
        } else {
            self.set_relation(name, LoadedRelation::Null);
        }
    }

    /// 安装已加载的模型集合，保留查询顺序和空集合。
    pub fn set_collection<M: HasRelationships>(
        &mut self,
        name: impl Into<String>,
        models: Vec<Arc<M>>,
    ) {
        self.insert(
            name,
            LoadedRelation::Related(Box::new(models)),
            Some(project_collection::<M>),
        );
    }

    pub(super) fn set_pivot(&mut self, name: &str, pivot: Pivot) {
        self.insert(
            name,
            LoadedRelation::Pivot(Box::new(pivot)),
            Some(project_pivot),
        );
    }

    pub(super) fn set_custom_pivot<P: CustomPivot>(&mut self, name: &str, pivot: P) {
        self.insert(
            name,
            LoadedRelation::Pivot(Box::new(pivot)),
            Some(project_custom_pivot::<P>),
        );
    }

    fn insert(
        &mut self,
        name: impl Into<String>,
        value: LoadedRelation,
        project: Option<ProjectRelation>,
    ) {
        self.entries
            .insert(name.into(), RelationEntry { value, project });
    }

    /// 移除指定关系的加载状态。
    pub fn unset_relation(&mut self, name: &str) {
        self.entries.shift_remove(name);
    }

    /// 依次列出 refresh 应重载的关系名，保留独立的中间表模型。
    pub fn refreshable_names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, entry)| !matches!(entry.value, LoadedRelation::Pivot(_)))
            .map(|(name, _)| name.as_str())
    }

    /// 投影已加载的可见关联，使用声明中的关系命名规则。
    pub fn relations_to_array(&self, metadata: &ModelMetadata) -> Result<ModelArray> {
        let mut values = ModelArray::new();
        for (name, entry) in &self.entries {
            if !metadata.attribute_is_arrayable(name) {
                continue;
            }
            let value = match (&entry.value, entry.project) {
                (LoadedRelation::Null, _) => ModelArrayValue::Attribute(Value::Null.into()),
                (value, Some(project)) => project(value)?,
                _ => continue,
            };
            let name = match metadata.relation_naming {
                RelationNaming::Snake => snake_relation(name),
                RelationNaming::Original => name.clone(),
            };
            values.insert(name, value);
        }
        Ok(values)
    }
}

fn snake_relation(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    let mut word_start = true;
    for mut character in name.chars() {
        if word_start {
            character = character.to_ascii_uppercase();
        }
        word_start = matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c');
        if character.is_whitespace() {
            continue;
        }
        if character.is_ascii_uppercase() && !output.is_empty() {
            output.push('_');
        }
        output.push(character);
    }
    output.to_lowercase()
}

fn typed_value<T: Any>(relation: &LoadedRelation) -> Result<&T> {
    relation.downcast_ref().ok_or_else(|| {
        Error::custom(
            "model_relation_type_invalid",
            "已加载关系与其模型投影类型不匹配",
        )
    })
}

fn project_model<M: HasRelationships>(value: &LoadedRelation) -> Result<ModelArrayValue> {
    typed_value::<Arc<M>>(value)?
        .to_array()
        .map(ModelArrayValue::Model)
}

fn project_collection<M: HasRelationships>(value: &LoadedRelation) -> Result<ModelArrayValue> {
    typed_value::<Vec<Arc<M>>>(value)?
        .iter()
        .map(|model| model.to_array())
        .collect::<Result<_>>()
        .map(ModelArrayValue::Collection)
}

fn project_pivot(value: &LoadedRelation) -> Result<ModelArrayValue> {
    let values = typed_value::<Pivot>(value)?
        .attributes()
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                ModelArrayValue::Attribute(value.clone().into()),
            )
        })
        .collect();
    Ok(ModelArrayValue::Model(values))
}

fn project_custom_pivot<P: CustomPivot>(value: &LoadedRelation) -> Result<ModelArrayValue> {
    let pivot = typed_value::<P>(value)?;
    project_model_array(pivot, |metadata| {
        pivot.pivot_relations().map_or_else(
            || Ok(ModelArray::new()),
            |relations| relations.relations_to_array(metadata),
        )
    })
    .map(ModelArrayValue::Model)
}

fn project_model_array<M: SaveModel + ?Sized>(
    model: &M,
    relations: impl FnOnce(&ModelMetadata) -> Result<ModelArray>,
) -> Result<ModelArray> {
    // getter 也可进入公开序列化方法；先登记本实例，再执行调用方代码。
    let visit = ArrayVisit::enter(std::ptr::from_ref(model).cast::<()>());
    let metadata = model.metadata();
    let values: ModelArray = model
        .attributes()
        .attributes_to_array(metadata)?
        .into_iter()
        .map(|(key, value)| (key, ModelArrayValue::Attribute(value)))
        .collect();
    if visit.is_none() {
        return Ok(values);
    }
    let children = relations(metadata)?;
    let mut numeric_index = 0_usize;
    Ok(values
        .into_iter()
        .chain(children)
        .map(|(key, value)| {
            let key = match ArrayKey::from(key) {
                ArrayKey::Integer(_) => {
                    let key = numeric_index.to_string();
                    numeric_index += 1;
                    key
                }
                ArrayKey::String(key) => key,
            };
            (key, value)
        })
        .collect())
}

pub(super) fn model_to_array<M: HasRelationships + ?Sized>(model: &M) -> Result<ModelArray> {
    project_model_array(model, |_| model.relations_to_array())
}

pub(super) fn model_to_json_value<M: HasRelationships + ?Sized>(model: &M) -> Result<Value> {
    array_json(model.to_array()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        eloquent::{AttributeGetter, AttributeMap, ModelAttributes},
        sea_orm,
    };
    use serde_json::{Map, json};
    use std::sync::Weak;

    struct Record {
        metadata: ModelMetadata,
        attributes: ModelAttributes,
        relations: ModelRelations,
    }

    impl Record {
        fn new(raw: Map<String, Value>) -> Self {
            Self {
                metadata: ModelMetadata::new("records", "Record"),
                attributes: ModelAttributes::new(raw),
                relations: ModelRelations::default(),
            }
        }
    }

    impl SaveModel for Record {
        fn metadata(&self) -> &ModelMetadata {
            &self.metadata
        }

        fn attributes(&self) -> &ModelAttributes {
            &self.attributes
        }

        fn attributes_mut(&mut self) -> &mut ModelAttributes {
            &mut self.attributes
        }

        fn exists(&self) -> bool {
            false
        }

        fn mark_created(&mut self) {}

        fn bind_attribute(&self, _: &str, _: &Value) -> Result<sea_orm::Value> {
            Err(Error::custom("unexpected_write", "序列化不应尝试写入"))
        }
    }

    impl HasRelationships for Record {
        fn relations(&self) -> &ModelRelations {
            &self.relations
        }

        fn relations_mut(&mut self) -> &mut ModelRelations {
            &mut self.relations
        }
    }

    #[test]
    fn model_array_merges_visible_loaded_relations_in_order() -> Result<()> {
        let mut parent = Record::new(Map::from_iter([
            ("first".into(), json!(1)),
            ("some_child".into(), json!("原属性")),
        ]));
        let mut child = Record::new(Map::from_iter([
            ("id".into(), json!(2)),
            ("secret".into(), json!("隐藏")),
        ]));
        child.metadata.hidden = vec!["secret".into()];
        child.relations.set_model::<Record>("emptyOwner", None);
        let child = Arc::new(child);
        parent.relations.set_model("someChild", Some(child.clone()));
        parent.relations.set_collection("childList", vec![child]);
        parent
            .relations
            .set_collection::<Record>("emptyList", Vec::new());
        parent
            .relations
            .set_relation("invalid", LoadedRelation::Related(Box::new(3_i64)));
        parent.relations.set_model::<Record>("hiddenRelation", None);
        parent
            .relations
            .set_model("blankModel", Some(Arc::new(Record::new(Map::new()))));
        let metadata = &mut parent.metadata;
        metadata.visible = [
            "first",
            "some_child",
            "someChild",
            "childList",
            "emptyList",
            "invalid",
            "hiddenRelation",
            "blankModel",
        ]
        .map(str::to_owned)
        .to_vec();
        metadata.hidden = vec!["hiddenRelation".into()];

        let array = parent.to_array()?;
        assert_eq!(
            array.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "first",
                "some_child",
                "child_list",
                "empty_list",
                "blank_model"
            ]
        );
        assert_eq!(
            serde_json::from_str::<Value>(&parent.to_json()?).map_err(Error::internal)?,
            json!({
                "first": 1,
                "some_child": {"id": 2, "empty_owner": null},
                "child_list": [{"id": 2, "empty_owner": null}],
                "empty_list": [],
                "blank_model": [],
            }),
        );
        parent.metadata.relation_naming = RelationNaming::Original;
        assert!(parent.json_serialize()?.get("someChild").is_some());
        assert_eq!(parent.json_serialize()?["some_child"], json!("原属性"));

        Ok(())
    }

    struct ParentProjection(Weak<Record>);

    impl AttributeGetter for ParentProjection {
        fn get(
            &self,
            _: Option<&AttributeValue>,
            _: &AttributeMap,
        ) -> Result<Option<AttributeValue>> {
            let parent = self
                .0
                .upgrade()
                .ok_or_else(|| Error::custom("test_parent_missing", "缺少父模型"))?;
            parent.json_serialize().map(|value| Some(value.into()))
        }
    }

    #[test]
    fn getter_reentry_uses_the_same_model_serialization_path() -> Result<()> {
        let mut registration = Ok(());
        let parent = Arc::new_cyclic(|parent_ref| {
            let mut parent = Record::new(Map::from_iter([("id".into(), json!(1))]));
            let mut child = Record::new(Map::new());
            child.metadata.append("parent");
            registration = child
                .attributes
                .register_getter("parent", Arc::new(ParentProjection(parent_ref.clone())));
            parent.relations.set_model("child", Some(Arc::new(child)));
            parent
        });
        registration?;
        let expected = json!({"id": 1, "child": {"parent": {"id": 1}}});
        assert_eq!(parent.json_serialize()?, expected);
        assert_eq!(parent.json_serialize()?, expected);
        Ok(())
    }
}
