//! 记录修改预览；使用公开属性快照，不承担数据库保存。
use crate::modules::example::{
    http::requests::example_record::ExampleRecordInput, models::example_record,
};
use openrust::Result;
use serde_json::{Map, Value};

#[derive(serde::Serialize)]
pub(crate) struct RecordPreview {
    pub(crate) original: Map<String, Value>,
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
    // metadata预览沿现update的类型保真JSON原值，不改框架PHP Array setter语义。
    attributes.set_raw_attribute(
        "metadata",
        example_record::Model::metadata_snapshot_value(input.metadata.as_ref())?,
    )?;
    let dirty = attributes.get_dirty(&metadata)?;
    // 发布的是此次预览的属性变化；没有SQL成功或sync_original的含义。
    attributes.sync_changes(&metadata)?;
    Ok(RecordPreview {
        original: attributes.get_raw_original_attributes()?,
        proposed: attributes.raw_attributes()?,
        dirty,
        changes: attributes.get_changes()?,
        previous: attributes.get_previous()?,
        has_changes: attributes.was_changed(&[])?,
        persisted: false,
    })
}

#[cfg(test)]
#[path = "record_preview_tests.rs"]
mod tests;
