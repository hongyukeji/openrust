//! 字段类型示例记录的账户隔离 CRUD。

use crate::modules::example::http::requests::example_record::{
    ExampleRecordInput, ExampleRecordPage,
};
use crate::modules::example::http::resources::example_record::ExampleRecordResource;
use crate::modules::example::models::example_record;
use openrust::auth::CurrentUser;
use openrust::database::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
};
use openrust::http::Created;
use openrust::http::Json;
use openrust::http::PageParams;
use openrust::http::Paginated;
use openrust::http::Path;
use openrust::http::Query;
use openrust::http::Validated;
use openrust::prelude::{Db, Inject};
use openrust::routing::Resource;
use openrust::routing::ResourceActions;
use openrust::{Error, Result};

pub(crate) struct ExampleRecords;

impl Resource for ExampleRecords {
    fn actions(actions: ResourceActions) -> ResourceActions {
        actions
            .list(index)
            .get(show)
            .create(store)
            .update(update)
            .delete(destroy)
    }
}

fn not_found() -> Error {
    Error::NotFound("example record not found".into())
}

pub(crate) async fn index(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Validated(Query(page)): Validated<Query<ExampleRecordPage>>,
) -> Result<Paginated<ExampleRecordResource>> {
    let connection = db.read_connection()?;
    let paginator = example_record::Entity::find()
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .order_by_asc(example_record::Column::Id)
        .paginate(&connection, page.per_page);
    let total = paginator.num_items().await.map_err(Error::internal)?;
    let rows = paginator
        .fetch_page(page.page - 1)
        .await
        .map_err(Error::internal)?;
    Ok(Paginated::new(
        rows.into_iter().map(ExampleRecordResource::from).collect(),
        PageParams {
            page: page.page,
            per_page: page.per_page,
        },
        total,
    ))
}

pub(crate) async fn show(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Path(id): Path<i64>,
) -> Result<Json<ExampleRecordResource>> {
    let model = example_record::Entity::find_by_id(id)
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .one(&db.read_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    Ok(Json(model.into()))
}

pub(crate) async fn store(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Created<Json<ExampleRecordResource>>> {
    let owner = user.integer_id()?;
    // 模块实体固定物理表名；复用已选主库池，不把框架表前缀施加到模块专有表。
    let query_db = Db::default();
    query_db.initialize(db.primary_connection()?)?;
    let model = db
        .transaction(move |transaction| {
            Box::pin(async move {
                let id = query_db
                    .table("example_records")?
                    .using_transaction(transaction)
                    .insert_get_id(
                        vec![
                            ("owner_id", owner.into()),
                            ("name", input.name.into()),
                            ("description", input.description.into()),
                            ("priority", input.priority.into()),
                            ("view_count", input.view_count.into()),
                            ("enabled", input.enabled.into()),
                            ("published_at", input.published_at.into()),
                            ("metadata", input.metadata.into()),
                            ("category_id", input.category_id.into()),
                        ],
                        None,
                    )
                    .await?;
                let openrust::database::sea_orm::Value::BigInt(Some(id)) = id else {
                    return Err(Error::custom(
                        "example_record_id_invalid",
                        "示例记录主键不是预期整数",
                    ));
                };
                example_record::Entity::find_by_id(id)
                    .filter(example_record::Column::OwnerId.eq(owner))
                    .one(transaction)
                    .await
                    .map_err(Error::internal)?
                    .ok_or_else(not_found)
            })
        })
        .await?;
    Ok(Created(Json(model.into())))
}

pub(crate) async fn update(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Path(id): Path<i64>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Json<ExampleRecordResource>> {
    let connection = db.primary_connection()?;
    let model = example_record::Entity::find_by_id(id)
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .one(&connection)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    let active = example_record::ActiveModel {
        id: Set(model.id),
        owner_id: Set(model.owner_id),
        name: Set(input.name),
        description: Set(input.description),
        priority: Set(input.priority),
        view_count: Set(input.view_count),
        enabled: Set(input.enabled),
        published_at: Set(input.published_at),
        metadata: Set(input.metadata),
        category_id: Set(input.category_id),
    };
    Ok(Json(
        active
            .update(&connection)
            .await
            .map_err(Error::internal)?
            .into(),
    ))
}

/// 预览当前账户记录的拟修改字段，不执行更新。
pub(crate) async fn preview(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Path(id): Path<i64>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Json<crate::modules::example::services::record_preview::RecordPreview>> {
    let model = example_record::Entity::find_by_id(id)
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    Ok(Json(
        crate::modules::example::services::record_preview::preview(&model, input).await?,
    ))
}

pub(crate) async fn destroy(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>> {
    let result = example_record::Entity::delete_many()
        .filter(example_record::Column::Id.eq(id))
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .exec(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?;
    if result.rows_affected != 1 {
        return Err(not_found());
    }
    Ok(Json(serde_json::json!({"deleted": true})))
}
