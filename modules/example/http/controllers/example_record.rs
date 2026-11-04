//! 字段类型示例记录的账户隔离 CRUD。

use crate::modules::example::http::requests::example_record::{
    ExampleRecordInput, ExampleRecordPage,
};
use crate::modules::example::http::resources::example_record::ExampleRecordResource;
use crate::modules::example::models::example_record;
use openrust::auth::CurrentUser;
use openrust::db::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
};
use openrust::http::{
    Created, Json, PageParams, Paginated, Path, Query, Resource, ResourceActions, Validated,
};
use openrust::prelude::{Db, Inject};
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
    let model = example_record::ActiveModel {
        owner_id: Set(user.integer_id()?),
        name: Set(input.name),
        description: Set(input.description),
        priority: Set(input.priority),
        view_count: Set(input.view_count),
        enabled: Set(input.enabled),
        published_at: Set(input.published_at),
        metadata: Set(input.metadata),
        category_id: Set(input.category_id),
        ..Default::default()
    }
    .insert(&db.primary_connection()?)
    .await
    .map_err(Error::internal)?;
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
