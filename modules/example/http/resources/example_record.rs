//! 示例记录响应；内部所有者标识不对外暴露。

use crate::modules::example::models::example_record;

#[derive(serde::Serialize)]
pub(crate) struct ExampleRecordResource {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) priority: i32,
    pub(crate) view_count: i64,
    pub(crate) enabled: bool,
    pub(crate) published_at: Option<openrust::db::sea_orm::prelude::DateTimeUtc>,
    pub(crate) metadata: Option<serde_json::Value>,
    pub(crate) category_id: Option<i64>,
}

impl From<example_record::Model> for ExampleRecordResource {
    fn from(model: example_record::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            description: model.description,
            priority: model.priority,
            view_count: model.view_count,
            enabled: model.enabled,
            published_at: model.published_at,
            metadata: model.metadata,
            category_id: model.category_id,
        }
    }
}
