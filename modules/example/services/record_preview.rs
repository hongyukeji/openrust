//! 记录修改预览；使用公开属性快照，不承担数据库保存。
use crate::modules::example::{
    http::requests::example_record::ExampleRecordInput, models::example_record,
};
use openrust::{
    Db, Error, Result,
    database::{
        SortDirection,
        eloquent::{
            ArrayObject, AsArrayObject, AsCollection, AsEnumCollection, AttributeCast,
            AttributeMap, AttributeSetter, AttributeValue, BelongsTo, BelongsToMany, EnumCast,
            HasMany, HasOne, HasRelationships, LoadedRelation, MassAssignment, ModelAttribute,
            ModelAttributes, ModelDateContext, ModelMetadata, PivotKeys, RelationLoad, SaveContext,
        },
    },
    support::{BoxFuture, Collection},
};
use serde_json::{Map, Value};
use std::sync::Arc;

#[derive(serde::Serialize)]
pub(crate) struct RecordPreview {
    pub(crate) editable_fields: Vec<String>,
    pub(crate) original: Map<String, Value>,
    pub(crate) original_transformed: Map<String, Value>,
    pub(crate) proposed: Map<String, Value>,
    pub(crate) display: RecordPreviewDisplay,
    pub(crate) dirty: Map<String, Value>,
    pub(crate) changes: Map<String, Value>,
    pub(crate) previous: Map<String, Value>,
    pub(crate) has_changes: bool,
    pub(crate) persisted: bool,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_relations: Option<OwnerRecordRelations>,
}

#[derive(serde::Serialize)]
pub(crate) struct OwnerRecordRelations {
    pub(crate) owner_record_ids: Vec<i64>,
    pub(crate) owner_latest_enabled_record_id: Option<i64>,
    pub(crate) record_category_name: Option<String>,
    pub(crate) record_category_links: Vec<RecordCategoryLink>,
}

#[derive(serde::Serialize)]
pub(crate) struct RecordCategoryLink {
    name: String,
    label: String,
}

#[derive(serde::Serialize)]
pub(crate) struct RecordPreviewDisplay {
    summary: Map<String, Value>,
    details: Map<String, Value>,
}

pub(crate) async fn preview(
    model: &example_record::Model,
    input: ExampleRecordInput,
    discard: bool,
    assignment: MassAssignment<'_>,
    dates: ModelDateContext,
) -> Result<RecordPreview> {
    let attributes = model.attribute_snapshot()?.with_date_context(dates);
    let mut metadata = example_record::Model::attribute_metadata();
    metadata.append("is_published");
    attributes.register_attribute(
        "is_published",
        ModelAttribute::new().get(|_, raw| {
            Ok(Value::Bool(
                raw.get("published_at")
                    .is_some_and(|value| !value.is_null()),
            )
            .into())
        }),
    )?;
    fill_record_input(&attributes, &metadata, input, assignment).await?;
    // 发布的是此次预览的属性变化；没有SQL成功或sync_original的含义。
    attributes.sync_changes(&metadata)?;
    if discard {
        attributes.discard_changes()?;
    }
    // 摘要与详情共用同一字段选择，详情读取转换后的值。
    let summary_fields = ["name", "priority", "enabled"];
    let project =
        |values: openrust::database::eloquent::AttributeMap| -> Result<Map<String, Value>> {
            values
                .into_iter()
                .map(|(name, value)| {
                    let value = attributes.project_attribute_json(&metadata, &name, value)?;
                    Ok((name, value))
                })
                .collect()
        };
    let mut details = attributes.attributes_to_json(&metadata)?;
    details.insert(
        "metadata_tag_count".into(),
        Value::from(metadata_tag_count(&attributes)?),
    );
    let priority_decimal = AttributeCast::decimal(2)
        .read(details.get("priority").cloned())?
        .unwrap_or(Value::Null);
    details.insert("priority_decimal".into(), priority_decimal);
    let status_flags = record_status_flags(&details)?;
    details.insert("status_flags".into(), status_flags);
    let summary = summary_fields
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                details.shift_remove(name).unwrap_or(Value::Null),
            )
        })
        .collect();
    let display = RecordPreviewDisplay { summary, details };
    let dirty = attributes.get_dirty(&metadata)?;
    Ok(RecordPreview {
        editable_fields: Vec::new(),
        original: attributes.get_raw_original_attributes()?,
        original_transformed: project(
            attributes.get_original_attribute_values(&metadata, |_| None)?,
        )?,
        proposed: attributes.raw_attributes()?,
        display,
        dirty,
        changes: attributes.get_changes()?,
        previous: attributes.get_previous()?,
        has_changes: attributes.was_changed(&[])?,
        persisted: false,
        owner_relations: None,
    })
}

pub(crate) enum RecordFlag {
    Enabled,
    Published,
}

pub(crate) fn record_flag_cast() -> EnumCast {
    EnumCast::unit(
        |name| match name {
            "Enabled" => Some(RecordFlag::Enabled),
            "Published" => Some(RecordFlag::Published),
            _ => None,
        },
        |case| match case {
            RecordFlag::Enabled => "Enabled",
            RecordFlag::Published => "Published",
        },
    )
}

fn record_status_flags(details: &Map<String, Value>) -> Result<Value> {
    let flags: Vec<_> = [("enabled", "Enabled"), ("is_published", "Published")]
        .into_iter()
        .filter_map(|(field, name)| {
            (details.get(field).and_then(Value::as_bool) == Some(true)).then_some(name)
        })
        .collect();
    let attributes = ModelAttributes::new(Map::from_iter([(
        "status_flags".into(),
        Value::String(serde_json::to_string(&flags).map_err(Error::internal)?),
    )]));
    let mut metadata = ModelMetadata::new("example_records", "RecordStatusFlags");
    metadata.casts.insert(
        "status_flags".into(),
        AsEnumCollection::of(record_flag_cast()),
    );
    attributes
        .attributes_to_json(&metadata)?
        .shift_remove("status_flags")
        .ok_or_else(|| Error::custom("example_status_flags_missing", "示例记录缺少状态标记"))
}

fn metadata_tag_count(attributes: &ModelAttributes) -> Result<usize> {
    let Some(raw) = attributes.raw_attribute("metadata")? else {
        return Ok(0);
    };
    let projected = ModelAttributes::new(Map::from_iter([("metadata".into(), raw)]));
    let mut metadata = ModelMetadata::new("example_records", "RecordMetadataPreview");
    metadata
        .casts
        .insert("metadata".into(), AsArrayObject::cast());
    metadata
        .casts
        .insert("tags".into(), AsCollection::default().cast());
    let Some(value) = projected.get_attribute_value(&metadata, "metadata")? else {
        return Ok(0);
    };
    let Some(object) = value.as_object() else {
        return Ok(0);
    };
    let snapshot = object.snapshot()?;
    let tags = snapshot
        .as_any()
        .downcast_ref::<ArrayObject>()
        .and_then(|metadata| metadata.as_collection().get("tags"));
    let Some(tags) = tags else {
        return Ok(0);
    };
    projected.set_raw_attribute(
        "tags",
        Value::String(serde_json::to_string(tags).map_err(Error::internal)?),
    )?;
    let Some(value) = projected.get_attribute_value(&metadata, "tags")? else {
        return Ok(0);
    };
    value.as_object().map_or(Ok(0), |object| {
        Ok(object
            .snapshot()?
            .as_any()
            .downcast_ref::<Collection>()
            .map_or(0, |tags| tags.all().len()))
    })
}

/// 查询已通过授权的记录所属账户下的关系。
pub(crate) async fn owner_relations(
    record: &example_record::Model,
    db: &Db,
    context: &SaveContext<'_>,
) -> Result<OwnerRecordRelations> {
    const RECORDS: &str = "records";
    const LATEST_ENABLED: &str = "latest_enabled_record";
    let related = example_record::RecordForSave::new(record.owner_id);
    let mut parent = example_record::RecordForSave::from_existing(record)?;
    // 保存快照省略所有者字段，此处使用已授权记录的所有者键加载关系。
    parent
        .attributes
        .set_raw_attribute("owner_id", Value::from(record.owner_id))?;
    let db = example_record::record_query_db(db)?;
    let relation = HasMany::new(&related, "owner_id", "owner_id");
    relation
        .load_on(
            std::slice::from_mut(&mut parent),
            RECORDS,
            context,
            &db,
            |query| {
                Ok(query
                    .select(&["id", "owner_id"])?
                    .order_by("id", SortDirection::Asc)?
                    .limit(10))
            },
        )
        .await?;
    HasOne::new(&related, "owner_id", "owner_id")
        .latest_of_many(None)?
        .where_candidates(|rows| rows.where_eq("enabled", true))
        .load_on(
            std::slice::from_mut(&mut parent),
            LATEST_ENABLED,
            context,
            &db,
            |query| query.select(&["id", "owner_id"]),
        )
        .await?;
    let record_category_name = record_category_name(record, &mut parent, &db, context).await?;
    let records = parent
        .relations()
        .get_relation(RECORDS)
        .and_then(|relation| relation.downcast_ref::<Vec<Arc<example_record::RecordForSave>>>())
        .ok_or_else(|| Error::custom("example_record_relation_unloaded", "示例账户记录未加载"))?;
    let owner_record_ids = records
        .iter()
        .map(|record| record.id())
        .collect::<Result<Vec<_>>>()?;
    let owner_latest_enabled_record_id = match parent.relations().get_relation(LATEST_ENABLED) {
        Some(LoadedRelation::Null) => None,
        Some(relation) => Some(
            relation
                .downcast_ref::<Arc<example_record::RecordForSave>>()
                .ok_or_else(|| {
                    Error::custom("example_record_relation_type", "示例单值关系类型不匹配")
                })?
                .id()?,
        ),
        None => {
            return Err(Error::custom(
                "example_record_relation_unloaded",
                "示例单值关系未加载",
            ));
        }
    };
    Ok(OwnerRecordRelations {
        owner_record_ids,
        owner_latest_enabled_record_id,
        record_category_name,
        record_category_links: record_category_links(&mut parent, &db, context).await?,
    })
}

pub(crate) fn category_links(
    category: &example_record::RecordCategory,
) -> BelongsToMany<'_, example_record::RecordCategory> {
    BelongsToMany::new(
        category,
        PivotKeys {
            table: "example_record_category_links",
            parent_foreign_key: "record_id",
            related_foreign_key: "category_id",
            parent_key: "id",
            related_key: "id",
        },
    )
    .with_pivot(&["label"])
}

async fn record_category_links(
    model: &mut example_record::RecordForSave,
    db: &Db,
    context: &SaveContext<'_>,
) -> Result<Vec<RecordCategoryLink>> {
    const CATEGORIES: &str = "supplementary_categories";
    let category = example_record::RecordCategory::new();
    category_links(&category)
        .load_on(
            std::slice::from_mut(model),
            RelationLoad {
                name: CATEGORIES,
                nested: &[],
            },
            context,
            db,
            |query| query.order_by("id", SortDirection::Asc),
        )
        .await?;
    let mut serialized = model.json_serialize()?;
    let Some(Value::Array(categories)) = serialized
        .as_object_mut()
        .and_then(|fields| fields.remove(CATEGORIES))
    else {
        return Err(Error::custom(
            "example_category_links_unloaded",
            "示例补充分类未加载",
        ));
    };
    categories
        .iter()
        .map(|category| {
            let name = category["name"].as_str().ok_or_else(|| {
                Error::custom("example_category_name_invalid", "示例分类名称类型无效")
            })?;
            let label = category["pivot"]["label"].as_str().ok_or_else(|| {
                Error::custom(
                    "example_category_link_label_missing",
                    "示例分类连接缺少标签",
                )
            })?;
            Ok(RecordCategoryLink {
                name: name.into(),
                label: label.into(),
            })
        })
        .collect()
}

async fn record_category_name(
    record: &example_record::Model,
    model: &mut example_record::RecordForSave,
    db: &Db,
    context: &SaveContext<'_>,
) -> Result<Option<String>> {
    const CATEGORY: &str = "category";
    model.attributes.set_raw_attribute(
        "category_id",
        record.category_id.map_or(Value::Null, Value::from),
    )?;
    BelongsTo::new(&example_record::RecordCategory::new(), "category_id", None)
        .load_on(std::slice::from_mut(model), CATEGORY, context, db, Ok)
        .await?;
    match model.relations().get_relation(CATEGORY) {
        Some(LoadedRelation::Null) => Ok(None),
        Some(relation) => relation
            .downcast_ref::<Arc<example_record::RecordCategory>>()
            .ok_or_else(|| Error::custom("example_record_relation_type", "示例分类关系类型不匹配"))?
            .name()
            .map(Some),
        None => Err(Error::custom(
            "example_record_relation_unloaded",
            "示例分类关系未加载",
        )),
    }
}

pub(crate) async fn fill_record_input(
    attributes: &ModelAttributes,
    metadata: &ModelMetadata,
    input: ExampleRecordInput,
    mut assignment: MassAssignment<'_>,
) -> Result<()> {
    assignment.prevent_silent_discard = true;
    // 此快照的业务 setter 区分省略 metadata 与显式 JSON null。
    attributes.register_setter(
        "metadata",
        Arc::new(MetadataPreviewSetter {
            present: input.metadata.is_some(),
        }),
    )?;
    let proposed = Map::from_iter(
        [
            ("name", Value::String(input.name)),
            ("description", Value::String(input.description)),
            ("priority", Value::from(input.priority)),
            ("view_count", Value::from(input.view_count)),
            ("enabled", Value::Bool(input.enabled)),
            (
                "published_at",
                serde_json::to_value(input.published_at).map_err(openrust::Error::internal)?,
            ),
            (
                "category_id",
                input.category_id.map_or(Value::Null, Value::from),
            ),
            ("metadata", input.metadata.unwrap_or(Value::Null)),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value)),
    );
    attributes.fill(metadata, proposed, &assignment).await?;
    Ok(())
}

// Option<Json>的None与显式Some(Null)沿现业务投影分别保留为null和文本"null"。
struct MetadataPreviewSetter {
    present: bool,
}
impl AttributeSetter for MetadataPreviewSetter {
    fn set(
        &self,
        value: AttributeValue,
        _raw_snapshot: AttributeMap,
    ) -> BoxFuture<'_, Result<AttributeMap>> {
        Box::pin(async move {
            let projected = example_record::Model::metadata_snapshot_value(
                self.present.then(|| value.as_json()).transpose()?,
            )?;
            Ok(AttributeMap::from_iter([(
                "metadata".into(),
                projected.into(),
            )]))
        })
    }
}

impl RecordPreview {
    pub(crate) fn with_schema_columns(mut self, columns: &[String]) -> Self {
        let metadata = example_record::Model::attribute_metadata();
        self.editable_fields = columns
            .iter()
            .filter(|name| metadata.get_fillable().contains(name))
            .cloned()
            .collect();
        self
    }
}

#[cfg(test)]
#[path = "record_preview_tests.rs"]
mod tests;
