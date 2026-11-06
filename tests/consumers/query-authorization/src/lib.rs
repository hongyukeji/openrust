//! 独立应用使用公开资源查询授权 API 的示例。

use openrust::{
    auth::{CurrentUser, QueryRule, QueryScope},
    core::{Cancellation, Result},
    database::{
        BatchDirection, BatchOptions, Db,
        sea_orm::{ColumnTrait, Condition, ConnectionTrait},
    },
};

pub mod document {
    use openrust::database::sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "documents")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub owner_id: i64,
        pub title: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// 只返回当前用户拥有且符合业务筛选的文档。
pub async fn visible_documents(
    user: &CurrentUser,
    connection: &impl ConnectionTrait,
    requested_id: Option<i64>,
) -> Result<Vec<document::Model>> {
    let rule = QueryRule::<document::Entity>::new(|user| {
        Ok(QueryScope::new(
            document::Column::OwnerId.eq(user.integer_id()?),
        ))
    });
    let filter = requested_id.map_or_else(Condition::all, |id| {
        Condition::all().add(document::Column::Id.eq(id))
    });
    rule.select(user, filter)?.all(connection).await
}

/// 逐批处理当前用户可见的文档，并在批次边界合作取消。
pub async fn visible_documents_in_batches(
    user: &CurrentUser,
    db: &Db,
    cancellation: &Cancellation,
) -> Result<Vec<document::Model>> {
    let rule = QueryRule::<document::Entity>::new(|user| {
        Ok(QueryScope::new(
            document::Column::OwnerId.eq(user.integer_id()?),
        ))
    });
    let mut documents = Vec::new();
    rule.select(user, Condition::all())?
        .into_batches()
        .run(
            db,
            BatchOptions::default(),
            BatchDirection::Ascending,
            cancellation,
            |batch| {
                documents.extend(batch);
                async { Ok(()) }
            },
        )
        .await?;
    Ok(documents)
}
