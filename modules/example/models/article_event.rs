//! 与文章写入同事务落库的模块业务事件。

use openrust::database::sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_article_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub owner_id: i64,
    pub article_id: i64,
    pub action: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
