//! 记录修改预览；使用公开属性快照，不承担数据库保存。
use crate::modules::example::{
    http::requests::example_record::ExampleRecordInput, models::example_record,
};
use openrust::{Result, database::eloquent::AttributeSetter, support::BoxFuture};
use serde_json::{Map, Value};

#[derive(serde::Serialize)]
pub(crate) struct RecordPreview {
    pub(crate) editable_fields: Vec<String>,
    pub(crate) original: Map<String, Value>,
    pub(crate) original_transformed: Map<String, Value>,
    pub(crate) proposed: Map<String, Value>,
    pub(crate) dirty: Map<String, Value>,
    pub(crate) changes: Map<String, Value>,
    pub(crate) previous: Map<String, Value>,
    pub(crate) has_changes: bool,
    pub(crate) persisted: bool,
}

pub(crate) async fn preview(
    model: &example_record::Model,
    input: ExampleRecordInput,
    discard: bool,
) -> Result<RecordPreview> {
    let attributes = model.attribute_snapshot()?;
    let metadata = example_record::Model::attribute_metadata();
    for (name, value) in [
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
    ] {
        attributes.set_attribute(&metadata, name, value).await?;
    }
    // 默认业务setter保留现update的JSON类型；注册的自定义setter仍优先。
    let default_setter = MetadataPreviewSetter {
        present: input.metadata.is_some(),
    };
    attributes
        .set_attribute_with_default_setter(
            &metadata,
            "metadata",
            input.metadata.unwrap_or(Value::Null),
            &default_setter,
        )
        .await?;
    // 发布的是此次预览的属性变化；没有SQL成功或sync_original的含义。
    attributes.sync_changes(&metadata)?;
    if discard {
        attributes.discard_changes()?;
    }
    let dirty = attributes.get_dirty(&metadata)?;
    Ok(RecordPreview {
        editable_fields: Vec::new(),
        original: attributes.get_raw_original_attributes()?,
        original_transformed: attributes.get_original_attributes(&metadata, |_| None)?,
        proposed: attributes.raw_attributes()?,
        dirty,
        changes: attributes.get_changes()?,
        previous: attributes.get_previous()?,
        has_changes: attributes.was_changed(&[])?,
        persisted: false,
    })
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
        self.editable_fields = columns
            .iter()
            .filter(|name| {
                matches!(
                    name.as_str(),
                    "name"
                        | "description"
                        | "priority"
                        | "view_count"
                        | "enabled"
                        | "published_at"
                        | "metadata"
                        | "category_id"
                )
            })
            .cloned()
            .collect();
        self
    }
}

#[cfg(test)]
#[path = "record_preview_tests.rs"]
mod tests;
