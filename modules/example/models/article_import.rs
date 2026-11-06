//! 异步文章导入的持久来源与终态收据。

use openrust::database::sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_article_imports")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub import_id: String,
    pub owner_id: i64,
    pub source_key: String,
    pub format: String,
    pub state: String,
    pub error_code: Option<String>,
    pub created_at_ms: i64,
    pub terminal_at_ms: Option<i64>,
    pub source_removed: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
