//! 记录修改预览；使用公开属性快照，不承担数据库保存。
use crate::modules::example::{
    http::requests::example_record::ExampleRecordInput, models::example_record,
};
use openrust::{
    Db, Error, Result,
    database::{
        SortDirection,
        eloquent::{
            AttributeSetter, HasMany, HasOne, HasRelationships, LoadedRelation, MassAssignment,
            ModelAttributes, ModelMetadata, SaveContext,
        },
    },
    support::BoxFuture,
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
) -> Result<RecordPreview> {
    let attributes = model.attribute_snapshot()?;
    let metadata = example_record::Model::attribute_metadata();
    fill_record_input(&attributes, &metadata, input, assignment).await?;
    // 发布的是此次预览的属性变化；没有SQL成功或sync_original的含义。
    attributes.sync_changes(&metadata)?;
    if discard {
        attributes.discard_changes()?;
    }
    // 摘要与详情共用同一字段选择，详情读取转换后的值。
    let summary_fields = ["name", "priority", "enabled"];
    let display = RecordPreviewDisplay {
        summary: attributes.only(&metadata, &summary_fields)?,
        details: attributes.except(&metadata, &summary_fields)?,
    };
    let dirty = attributes.get_dirty(&metadata)?;
    Ok(RecordPreview {
        editable_fields: Vec::new(),
        original: attributes.get_raw_original_attributes()?,
        original_transformed: attributes.get_original_attributes(&metadata, |_| None)?,
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

/// 查询已通过授权的记录所属账户下的关系。
pub(crate) async fn owner_relations(
    record: &example_record::Model,
    db: &Db,
    context: &SaveContext<'_>,
) -> Result<OwnerRecordRelations> {
    const RECORDS: &str = "records";
    const LATEST_ENABLED: &str = "latest_enabled_record";
    let related = example_record::RecordForSave::new(record.owner_id);
    let mut parent = example_record::RecordForSave::new(record.owner_id);
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
    })
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
        value: Value,
        _raw_snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            let projected =
                example_record::Model::metadata_snapshot_value(self.present.then_some(&value))?;
            Ok(Map::from_iter([("metadata".into(), projected)]))
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
