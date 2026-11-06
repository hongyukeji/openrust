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
