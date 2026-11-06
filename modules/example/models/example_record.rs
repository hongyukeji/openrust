//! 字段类型演示实体，使用专属的 `example_records` 表。

use openrust::database::sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_records")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
    pub description: String,
    pub priority: i32,
    pub view_count: i64,
    pub enabled: bool,
    pub published_at: Option<DateTimeUtc>,
    pub metadata: Option<Json>,
    pub category_id: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// 以当前业务字段建立独立预览快照，不改变实体或数据库记录。
    pub(crate) fn attribute_snapshot(
        &self,
    ) -> openrust::Result<openrust::database::eloquent::ModelAttributes> {
        let metadata = Self::metadata_snapshot_value(self.metadata.as_ref())?;
        Ok(openrust::database::eloquent::ModelAttributes::new(
            serde_json::Map::from_iter([
                ("name".into(), serde_json::json!(self.name)),
                ("description".into(), serde_json::json!(self.description)),
                ("priority".into(), serde_json::json!(self.priority)),
                ("view_count".into(), serde_json::json!(self.view_count)),
                ("enabled".into(), serde_json::json!(self.enabled)),
                (
                    "published_at".into(),
                    serde_json::to_value(self.published_at).map_err(openrust::Error::internal)?,
                ),
                ("metadata".into(), metadata),
                ("category_id".into(), serde_json::json!(self.category_id)),
            ]),
        ))
    }
    // 和现有SeaORM更新共享serde JSON值投影，保留整double的.0及原键顺序。
    pub(crate) fn metadata_snapshot_value(
        value: Option<&Json>,
    ) -> openrust::Result<serde_json::Value> {
        value
            .map(serde_json::to_string)
            .transpose()
            .map_err(openrust::Error::internal)
            .map(|text| text.map_or(serde_json::Value::Null, serde_json::Value::String))
    }
    pub(crate) fn attribute_metadata() -> openrust::database::eloquent::ModelMetadata {
        use openrust::database::eloquent::{AttributeCast, ModelMetadata};
        let mut metadata = ModelMetadata::new(
            "example_records",
            "modules::example::models::example_record::Model",
        );
        metadata.casts = [
            ("priority".into(), AttributeCast::Integer),
            ("view_count".into(), AttributeCast::Integer),
            ("enabled".into(), AttributeCast::Boolean),
            ("metadata".into(), AttributeCast::Array),
        ]
        .into_iter()
        .collect();
        metadata
    }
}
