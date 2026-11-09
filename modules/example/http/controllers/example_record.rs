//! 字段类型示例记录的账户隔离 CRUD。

use crate::modules::example::http::requests::example_record::{
    ExampleRecordInput, ExampleRecordPage, ExampleRecordPreviewOptions,
};
use crate::modules::example::http::resources::example_record::ExampleRecordResource;
use crate::modules::example::models::example_record::{self, record_query_db};
use crate::modules::example::services::record_preview::fill_record_input;
use openrust::auth::CurrentUser;
use openrust::database::eloquent::{
    MassAssignment, ModelDateContext, SaveContext, save_with_query,
};
use openrust::database::sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter};
use openrust::events::Dispatcher;
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

fn invalid_metadata_filter(message: &str) -> Error {
    let mut errors = openrust::ValidationErrors::new();
    errors.add("metadata_contains", message);
    Error::Validation(errors)
}

pub(crate) async fn index(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Validated(Query(page)): Validated<Query<ExampleRecordPage>>,
) -> Result<Paginated<ExampleRecordResource>> {
    // 模块使用既有物理表名；固定原读连接，不把应用表前缀套到模块表。
    let query_db = Db::default();
    query_db.initialize(db.read_connection()?)?;
    let mut query = query_db
        .table("example_records")?
        .where_eq("owner_id", user.integer_id()?)?;
    if let Some(text) = page.metadata_contains.as_deref() {
        let value = serde_json::from_str(text)
            .map_err(|_| invalid_metadata_filter("元数据包含条件必须是有效JSON"))?;
        query = query
            .where_json_contains("metadata", value)
            .map_err(|error| {
                if error.code() == "query_sqlite_json_contains_requires_scalar" {
                    invalid_metadata_filter("SQLite 元数据包含条件必须是 JSON 标量")
                } else {
                    error
                }
            })?;
    }
    if let Some(tag) = page.metadata_tag.as_deref() {
        let tag = serde_json::json!(tag);
        query = query.where_nested(|group| {
            group
                .where_json_contains("example_records.metadata->tags", tag.clone())?
                .or_where_json_contains("example_records.metadata->labels", tag)
        })?;
    }
    let result = query
        .order_by("id", openrust::database::query::SortDirection::Asc)?
        .paginate_on(page.per_page, "page", page.page)
        .await?;
    let rows = result
        .items
        .iter()
        .map(|row| example_record::Model::from_query_result(row, "").map_err(Error::internal))
        .collect::<Result<Vec<_>>>()?;
    Ok(Paginated::new(
        rows.into_iter().map(ExampleRecordResource::from).collect(),
        PageParams {
            page: result.current_page,
            per_page: result.per_page,
        },
        result.total,
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

#[cfg(feature = "encryption")]
const PRIVATE_METADATA: &str = "private_metadata";

#[cfg(feature = "encryption")]
fn private_metadata_model(
    encrypter: std::sync::Arc<dyn openrust::encryption::ApplicationEncrypter>,
) -> openrust::database::eloquent::ModelMetadata {
    use openrust::database::eloquent::{
        AttributeCast, EncryptedCast, EncryptedCastKind, ModelMetadata,
    };
    let mut model = ModelMetadata::new("example_records", "RecordPrivateMetadata");
    model.casts.insert(
        PRIVATE_METADATA.into(),
        AttributeCast::Encrypted(EncryptedCast::new(EncryptedCastKind::Json, encrypter)),
    );
    model
}

#[cfg(feature = "encryption")]
pub(crate) async fn put_private_metadata(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(encrypter): Inject<dyn openrust::encryption::ApplicationEncrypter>,
    Path(id): Path<i64>,
    Json(input): Json<serde_json::Map<String, serde_json::Value>>,
) -> Result<http::StatusCode> {
    use openrust::database::{eloquent::ModelAttributes, sea_orm::Value};
    let model = private_metadata_model(encrypter);
    let attributes = ModelAttributes::default();
    attributes
        .set_attribute(&model, PRIVATE_METADATA, serde_json::Value::Object(input))
        .await?;
    let ciphertext = attributes
        .raw_attribute(PRIVATE_METADATA)?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            Error::custom("example_private_metadata_missing", "示例私有元数据缺少密文")
        })?;
    let updated = record_query_db(&db)?
        .table(&model.table)?
        .where_eq("id", id)?
        .where_eq("owner_id", user.integer_id()?)?
        .update(vec![(PRIVATE_METADATA, Value::from(ciphertext))])
        .await?;
    if updated == 0 {
        return Err(not_found());
    }
    Ok(http::StatusCode::NO_CONTENT)
}

#[cfg(feature = "encryption")]
#[derive(serde::Deserialize)]
pub(crate) struct PrivateMetadataPath {
    path: String,
    value: serde_json::Value,
}

#[cfg(feature = "encryption")]
pub(crate) async fn patch_private_metadata(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(encrypter): Inject<dyn openrust::encryption::ApplicationEncrypter>,
    Path(id): Path<i64>,
    Json(input): Json<PrivateMetadataPath>,
) -> Result<http::StatusCode> {
    use openrust::database::{eloquent::ModelAttributes, sea_orm::Value};
    use serde_json::{Map, Value as JsonValue};

    let owner = user.integer_id()?;
    let model = private_metadata_model(encrypter);
    let query_db = record_query_db(&db)?;
    db.transaction_for_update_checked(
        move |transaction| {
            Box::pin(async move {
                let query = query_db
                    .table(&model.table)?
                    .using_transaction(transaction)
                    .where_eq("id", id)?
                    .where_eq("owner_id", owner)?
                    .lock_for_update()?;
                let raw = query
                    .value::<Option<String>>(PRIVATE_METADATA)
                    .await?
                    .ok_or_else(not_found)?;
                let attributes = ModelAttributes::new(Map::from_iter([(
                    PRIVATE_METADATA.into(),
                    raw.map_or(JsonValue::Null, JsonValue::String),
                )]));
                let path = format!("{PRIVATE_METADATA}->{}", input.path);
                attributes
                    .set_attribute(&model, &path, input.value)
                    .await
                    .map_err(|error| match error.code() {
                        "encryption_payload_invalid"
                        | "encryption_payload_mac_invalid"
                        | "encryption_payload_tag_for_cbc"
                        | "encryption_payload_decrypt_failed" => {
                            Error::internal(std::io::Error::other("示例私有元数据的密文无效"))
                        }
                        _ => error,
                    })?;
                let ciphertext = attributes
                    .raw_attribute(PRIVATE_METADATA)?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or_else(|| {
                        Error::custom("example_private_metadata_missing", "示例私有元数据缺少密文")
                    })?;
                query
                    .update(vec![(PRIVATE_METADATA, Value::from(ciphertext))])
                    .await?;
                Ok(())
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await?;
    Ok(http::StatusCode::NO_CONTENT)
}

#[cfg(feature = "encryption")]
pub(crate) async fn get_private_metadata(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(encrypter): Inject<dyn openrust::encryption::ApplicationEncrypter>,
    Path(id): Path<i64>,
) -> Result<openrust::http::axum::response::Response> {
    use openrust::http::axum::{
        http::{HeaderValue, header},
        response::IntoResponse,
    };
    let model = private_metadata_model(encrypter);
    let raw = record_query_db(&db)?
        .table(&model.table)?
        .where_eq("id", id)?
        .where_eq("owner_id", user.integer_id()?)?
        .value::<Option<String>>(PRIVATE_METADATA)
        .await?
        .flatten()
        .ok_or_else(not_found)?;
    let value = model
        .cast_attribute(PRIVATE_METADATA, Some(serde_json::Value::String(raw)))
        .map_err(|_| Error::internal(std::io::Error::other("示例私有元数据的密文无效")))?;
    let mut response = Json(value).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

pub(crate) async fn store(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(databases): Inject<Databases>,
    Inject(events): Inject<Dispatcher>,
    Extension(calendar): Extension<CalendarContext>,
    Validated(Json(input)): Validated<Json<ExampleRecordInput>>,
) -> Result<Created<Json<ExampleRecordResource>>> {
    let owner = user.integer_id()?;
    let query_db = record_query_db(&db)?;
    let model = db
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut model = example_record::RecordForSave::new(owner);
                model.attributes =
                    model
                        .attributes
                        .with_date_context(ModelDateContext::for_connection(
                            calendar.clone(),
                            &query_db,
                        )?);
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
    Inject(events): Inject<Dispatcher>,
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
    model.attributes = model
        .attributes
        .with_date_context(ModelDateContext::for_connection(calendar.clone(), &db)?);
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
    (Inject(events), Extension(calendar)): (Inject<Dispatcher>, Extension<CalendarContext>),
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
    let mut preview = crate::modules::example::services::record_preview::preview(
        &model,
        input,
        options.discard,
        MassAssignment::new(&databases),
        ModelDateContext::for_connection(calendar.clone(), &db)?,
    )
    .await?
    .with_schema_columns(&columns);
    if options.include_owner_records {
        let context = SaveContext::new(&databases, Some(&events), &calendar);
        preview.owner_relations = Some(
            crate::modules::example::services::record_preview::owner_relations(
                &model, &db, &context,
            )
            .await?,
        );
    }
    Ok(Json(preview))
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
