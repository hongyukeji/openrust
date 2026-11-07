//! 字段类型示例记录的账户隔离 CRUD。

use crate::modules::example::http::requests::example_record::{
    ExampleRecordInput, ExampleRecordPage, ExampleRecordPreviewOptions,
};
use crate::modules::example::http::resources::example_record::ExampleRecordResource;
use crate::modules::example::models::example_record;
use crate::modules::example::services::record_preview::fill_record_input;
use openrust::auth::CurrentUser;
use openrust::database::eloquent::{MassAssignment, SaveContext, save_with_query};
use openrust::database::sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};
use openrust::events::Events;
use openrust::http::Created;
use openrust::http::Json;
use openrust::http::PageParams;
use openrust::http::Paginated;
use openrust::http::Path;
use openrust::http::Query;
use openrust::http::Validated;
use openrust::http::axum::Extension;
use openrust::prelude::{Db, Inject};
use openrust::routing::Resource;
use openrust::routing::ResourceActions;
use openrust::support::CalendarContext;
use openrust::{Databases, Error, Result};

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
    Inject(databases): Inject<Databases>,
    Inject(events): Inject<Events>,
    Extension(calendar): Extension<CalendarContext>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Created<Json<ExampleRecordResource>>> {
    let owner = user.integer_id()?;
    let query_db = record_query_db(&db)?;
    let model = db
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut model = example_record::RecordForSave::new(owner);
                fill_record_input(
                    &model.attributes,
                    &model.metadata,
                    input,
                    MassAssignment::new(&databases),
                )
                .await?;
                let context = SaveContext {
                    databases: &databases,
                    events: Some(&events),
                    calendar: &calendar,
                    touch: true,
                };
                let query = query_db
                    .table("example_records")?
                    .using_transaction(transaction);
                if !save_with_query(&mut model, &context, query).await? {
                    return Err(save_cancelled());
                }
                example_record::Entity::find_by_id(model.id()?)
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
    Inject(databases): Inject<Databases>,
    Inject(events): Inject<Events>,
    Extension(calendar): Extension<CalendarContext>,
    Path(id): Path<i64>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Json<ExampleRecordResource>> {
    let connection = db.primary_connection()?;
    let owner = user.integer_id()?;
    let selected =
        example_record::Entity::find_by_id(id).filter(example_record::Column::OwnerId.eq(owner));
    let existing = selected
        .clone()
        .one(&connection)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    let mut model = example_record::RecordForSave::from_existing(&existing)?;
    fill_record_input(
        &model.attributes,
        &model.metadata,
        input,
        MassAssignment::new(&databases),
    )
    .await?;
    let context = SaveContext {
        databases: &databases,
        events: Some(&events),
        calendar: &calendar,
        touch: true,
    };
    let query = record_query_db(&db)?
        .table("example_records")?
        .where_eq("owner_id", owner)?;
    if !save_with_query(&mut model, &context, query).await? {
        return Err(save_cancelled());
    }
    let updated = selected
        .one(&connection)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    Ok(Json(updated.into()))
}

fn record_query_db(db: &Db) -> Result<Db> {
    // 模块表名固定；查询借用同一主库连接，沿用示例现有的无前缀映射。
    let query_db = Db::default();
    query_db.initialize(db.primary_connection()?)?;
    Ok(query_db)
}

fn save_cancelled() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::CONFLICT,
        "example_record_save_cancelled",
        "示例记录保存已取消",
    )
}

/// 预览当前账户记录的拟修改字段，不执行更新。
pub(crate) async fn preview(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(databases): Inject<Databases>,
    Path(id): Path<i64>,
    Query(options): Query<ExampleRecordPreviewOptions>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Json<crate::modules::example::services::record_preview::RecordPreview>> {
    let model = example_record::Entity::find_by_id(id)
        .filter(example_record::Column::OwnerId.eq(user.integer_id()?))
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(not_found)?;
    // 先owner筛选/404，再读真实列清单；此处不替代Validated或Model guardable。
    let columns = db.schema().get_column_listing("example_records").await?;
    Ok(Json(
        crate::modules::example::services::record_preview::preview(
            &model,
            input,
            options.discard,
            MassAssignment::new(&databases),
        )
        .await?
        .with_schema_columns(&columns),
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
