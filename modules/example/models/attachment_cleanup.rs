//! 文章删除后的持久对象清理意图。

use openrust::db::sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "example_attachment_cleanup")]
#[allow(clippy::struct_field_names)] // 列名与持久表一致，迁移和 Entity 查询均依赖这些键。
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub article_id: i64,
    pub owner_id: i64,
    pub tenant_id: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
