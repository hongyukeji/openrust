//! 验证生成应用的 Query Builder 插入 ID 与 Eloquent 保存。

use openrust::{
    Error, Result,
    database::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, TransactionTrait, Value},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

fn check(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom(
            "generated_insert_id_contract",
            "插入 ID 公开契约不符",
        ))
    }
}

#[tokio::test]
async fn generated_application_insert_ids_preserve_execution_and_backend_contracts()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    // {{database_setup}}
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let connection = db.connection()?;
        let backend = db.backend()?;
        let ddl = match backend {
            DbBackend::Sqlite => "CREATE TABLE insert_items (id INTEGER PRIMARY KEY, label TEXT DEFAULT 'default')",
            DbBackend::MySql => "CREATE TABLE insert_items (id BIGINT AUTO_INCREMENT PRIMARY KEY, label VARCHAR(255) DEFAULT 'default')",
            DbBackend::Postgres => "CREATE TABLE insert_items (id BIGSERIAL PRIMARY KEY, label TEXT DEFAULT 'default')",
            _ => return Err(Error::custom("generated_insert_id_backend", "测试需要受支持的数据库")),
        };
        connection.execute_unprepared(ddl).await.map_err(Error::internal)?;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let query = db.table("insert_items")?.before_query(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        check(query.insert_get_id(Vec::new(), None).await? == Value::BigInt(Some(1)))?;
        let label = "bound ' value ? $1";
        check(query.insert_get_id(vec![("label", label.into())], Some("")).await? == Value::BigInt(Some(2)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        check(query.count().await? == 2)?;
        check(db.table("insert_items")?.where_eq("id", 2_i64)?.value::<String>("label").await?.as_deref() == Some(label))?;
        check(db.table("insert_items")?.where_eq("id", 1_i64)?.value::<String>("label").await?.as_deref() == Some("default"))?;
        check(query.insert_get_id(Vec::new(), Some("0")).await? == Value::BigInt(Some(3)))?;
        check(calls.load(Ordering::SeqCst) == 1)?;
        rollback_insert(&db).await?;
        check(query.count().await? == 3)?;
        backend_ids(&db).await?;
        generated_model_save(database.url()).await
    }.await;
    database.close().await?;
    outcome?;
    Ok(())
}

async fn rollback_insert(db: &Db) -> Result<()> {
    let transaction = db.connection()?.begin().await.map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let query = db.table("insert_items")?.using_transaction(&transaction);
        check(
            query
                .insert_get_id(vec![("label", "rolled-back".into())], None)
                .await?
                == Value::BigInt(Some(4)),
        )?;
        check(query.count().await? == 4)
    }
    .await;
    transaction.rollback().await.map_err(Error::internal)?;
    outcome
}

async fn backend_ids(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    match db.backend()? {
        DbBackend::Sqlite => {
            connection
                .execute_unprepared("CREATE TABLE signed_insert_items (id INTEGER PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("signed_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("id", (-10_i64).into())], None)
                    .await?
                    == Value::BigInt(Some(-10)),
            )?;
            check(
                query
                    .insert_get_id(Vec::new(), Some("ignored_sequence"))
                    .await?
                    == Value::BigInt(Some(-9)),
            )
        }
        DbBackend::Postgres => {
            connection
                .execute_unprepared("CREATE TABLE text_insert_items (custom_key TEXT PRIMARY KEY)")
                .await
                .map_err(Error::internal)?;
            let query = db.table("text_insert_items")?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "000123".into())], Some("custom_key"))
                    .await?
                    == Value::BigInt(Some(123)),
            )?;
            check(
                query
                    .insert_get_id(vec![("custom_key", "key:abc".into())], Some("custom_key"))
                    .await?
                    == Value::from("key:abc"),
            )
        }
        DbBackend::MySql => check(
            db.table("insert_items")?
                .insert_get_id(Vec::new(), Some("ignored_sequence"))
                .await?
                == Value::BigInt(Some(5)),
        ),
        _ => Err(Error::custom(
            "generated_insert_id_backend",
            "测试需要受支持的数据库",
        )),
    }
}

struct GeneratedModel {
    metadata: openrust::database::eloquent::ModelMetadata,
    attributes: openrust::database::eloquent::ModelAttributes,
    exists: bool,
}

impl openrust::database::eloquent::SaveModel for GeneratedModel {
    fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
        &self.metadata
    }

    fn attributes(&self) -> &openrust::database::eloquent::ModelAttributes {
        &self.attributes
    }

    fn attributes_mut(&mut self) -> &mut openrust::database::eloquent::ModelAttributes {
        &mut self.attributes
    }

    fn exists(&self) -> bool {
        self.exists
    }

    fn mark_created(&mut self) {
        self.exists = true;
    }

    fn bind_attribute(&self, name: &str, value: &serde_json::Value) -> Result<Value> {
        match name {
            "id" => value.as_i64().map(Value::from),
            "label" => value.as_str().map(Value::from),
            _ => None,
        }
        .ok_or_else(|| Error::custom("generated_model_binding", "生成应用模型列类型错误"))
    }
}

impl openrust::database::eloquent::HydrateModel for GeneratedModel {
    fn hydrate(&self, raw: serde_json::Map<String, serde_json::Value>) -> Result<Self> {
        Ok(Self {
            metadata: self.metadata.clone(),
            attributes: openrust::database::eloquent::ModelAttributes::new(raw),
            exists: true,
        })
    }
}

async fn generated_model_save(url: &str) -> Result<()> {
    use openrust::database::{
        DatabaseServiceProvider,
        eloquent::{MassAssignment, ModelMetadata, SaveContext, refresh, save, save_or_fail},
    };
    use openrust::support::CalendarContext;
    use serde_json::{Map, json};

    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);
    builder.provider(generated_retrieved_events());
    let source = figment::Figment::new()
        .merge(("database.url", url))
        .merge(("database.instances.other.url", url));
    let runtime = builder.start(source).await?;
    let outcome = async {
        let databases = runtime.container().resolve::<openrust::Databases>().await?;
        let db = databases.default_database();
        let calendar = CalendarContext::default();
        let events = runtime
            .container()
            .resolve::<openrust::events::Events>()
            .await?;
        let context = SaveContext::new(&databases, Some(&events), &calendar);
        let mut metadata = ModelMetadata::new("insert_items", "generated::InsertItem");
        metadata.timestamps = false;
        metadata.fillable(vec!["label".into()]);
        let mut model = GeneratedModel {
            metadata,
            attributes: Default::default(),
            exists: false,
        };
        model
            .attributes
            .fill(
                &model.metadata,
                Map::from_iter([("label".into(), json!("Eloquent 创建"))]),
                &MassAssignment::new(&databases),
            )
            .await?;
        check(save(&mut model, &context).await? && model.exists)?;
        let original = model
            .attributes
            .raw_attribute("id")?
            .and_then(|value| value.as_i64())
            .ok_or_else(|| Error::custom("generated_model_id_missing", "保存未写回自增主键"))?;
        let replacement = original
            .checked_add(100)
            .ok_or_else(|| Error::custom("generated_model_id_range", "生成应用测试主键超出范围"))?;
        model
            .attributes
            .set_raw_attribute("id", json!(replacement))?;
        model
            .attributes
            .set_raw_attribute("label", json!("Eloquent 更新"))?;
        check(save_or_fail(&mut model, &context).await?)?;
        check(model.attributes.get_previous()?.get("id") == Some(&json!(original)))?;
        check(model.attributes.is_clean(&model.metadata, &[])?)?;
        check(
            db.table("insert_items")?
                .where_eq("id", original)?
                .count()
                .await?
                == 0,
        )?;
        check(
            db.table("insert_items")?
                .where_eq("id", replacement)?
                .value::<String>("label")
                .await?
                .as_deref()
                == Some("Eloquent 更新"),
        )?;
        let changes = model.attributes.get_changes()?;
        model
            .attributes
            .set_raw_attribute("label", json!("内存草稿"))?;
        db.table("insert_items")?
            .where_eq("id", replacement)?
            .update(vec![("label", "外部更新".into())])
            .await?;
        refresh(&mut model, &context).await?;
        check(model.attributes.get_raw_original("label")? == Some(json!("监听器读取")))?;
        check(model.attributes.is_clean(&model.metadata, &[])?)?;
        check(model.attributes.get_changes()? == changes)?;
        check_fresh_model(&model, &context).await?;
        check_failed_refresh(&db, &mut model, &context, replacement).await?;
        let mut absent = GeneratedModel {
            metadata: model.metadata.clone(),
            attributes: openrust::database::eloquent::ModelAttributes::new(Map::from_iter([
                ("id".into(), json!(original)),
                ("label".into(), json!("原基线")),
            ])),
            exists: true,
        };
        absent
            .attributes
            .set_raw_attribute("label", json!("本地草稿"))?;
        generated_locked_model_refresh(Arc::clone(&databases), Arc::clone(&events), model).await?;
        let missing = refresh(&mut absent, &context).await;
        check(matches!(missing, Err(error) if error.code() == "model_not_found"))?;
        check(absent.attributes.get_raw_original("label")? == Some(json!("原基线")))?;
        check(absent.attributes.raw_attribute("label")? == Some(json!("本地草稿")))
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

async fn generated_locked_model_refresh(
    databases: Arc<openrust::Databases>,
    events: Arc<openrust::events::Events>,
    mut model: GeneratedModel,
) -> Result<()> {
    use openrust::{
        database::eloquent::{SaveContext, refresh_for_update},
        support::CalendarContext,
    };
    use serde_json::json;

    let db = databases.default_database();
    model
        .attributes
        .set_raw_attribute("label", json!("锁定前草稿"))?;
    db.transaction_for_update_checked_context(
        move |transaction| {
            Box::pin(async move {
                let calendar = CalendarContext::default();
                let context = SaveContext::new(&databases, Some(&events), &calendar);
                model.metadata.connection = Some("other".into());
                let wrong_source = refresh_for_update(&mut model, &context, &transaction).await;
                check(matches!(wrong_source, Err(error)
                    if error.code() == "model_refresh_transaction_source_mismatch"))?;
                check(model.attributes.raw_attribute("label")? == Some(json!("锁定前草稿")))?;
                model.metadata.connection = None;
                refresh_for_update(&mut model, &context, &transaction).await?;
                check(
                    model.attributes.get_raw_original("label")? == Some(json!("事务内监听器读取")),
                )?;
                check(model.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))?;
                check(model.attributes.is_clean(&model.metadata, &[])?)
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await
}

fn generated_retrieved_events() -> openrust::events::EventsServiceProvider {
    let mut provider = openrust::events::EventsServiceProvider::new();
    provider.listen_named_reply(
        "eloquent.retrieved: generated::InsertItem",
        |payload, context| {
            Box::pin(async move {
                let retrieved = payload.downcast_ref::<GeneratedModel>().ok_or_else(|| {
                    Error::custom("generated_retrieved_type", "retrieved 事件模型类型不符")
                })?;
                let label = retrieved.attributes.get_raw_original("label")?;
                let transaction = context.transaction().is_some();
                if label == Some(serde_json::json!("监听器拒绝")) {
                    retrieved
                        .attributes
                        .set_raw_attribute("label", serde_json::json!("失败前的临时修改"))?;
                    return Err(Error::custom(
                        "generated_retrieved_failed",
                        "回读监听器失败",
                    ));
                }
                check(retrieved.exists)?;
                check(label == retrieved.attributes.raw_attribute("label")?)?;
                let loaded = if transaction {
                    "事务内监听器读取"
                } else {
                    "监听器读取"
                };
                retrieved
                    .attributes
                    .set_raw_attribute("label", serde_json::json!(loaded))?;
                Ok(openrust::support::DataValue::Null)
            })
        },
    );
    provider
}

async fn check_failed_refresh(
    db: &Db,
    model: &mut GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
    key: i64,
) -> Result<()> {
    use openrust::database::eloquent::{fresh, refresh};
    use serde_json::json;

    let query = || db.table("insert_items")?.where_eq("id", key);
    check(query()?.value::<String>("label").await?.as_deref() == Some("外部更新"))?;
    query()?
        .update(vec![("label", "监听器拒绝".into())])
        .await?;
    let before = model.attributes.raw_attributes()?;
    let changes = model.attributes.get_changes()?;
    let previous = model.attributes.get_previous()?;
    let failed_refresh = refresh(model, context).await;
    check(matches!(failed_refresh, Err(error) if error.code() == "generated_retrieved_failed"))?;
    let failed_fresh = fresh(model, context).await;
    check(matches!(failed_fresh, Err(error) if error.code() == "generated_retrieved_failed"))?;
    check(model.attributes.raw_attributes()? == before)?;
    check(model.attributes.get_raw_original("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.get_changes()? == changes)?;
    check(model.attributes.get_previous()? == previous)?;
    check(query()?.value::<String>("label").await?.as_deref() == Some("监听器拒绝"))?;
    query()?.update(vec![("label", "外部更新".into())]).await?;
    Ok(())
}

async fn check_fresh_model(
    model: &GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::eloquent::fresh;
    use serde_json::json;

    model
        .attributes
        .set_raw_attribute("label", json!("原对象草稿"))?;
    let fresh = fresh(model, context)
        .await?
        .ok_or_else(|| Error::custom("generated_model_fresh_missing", "回读未找到模型"))?;
    check(fresh.exists)?;
    check(fresh.attributes.get_raw_original("label")? == Some(json!("外部更新")))?;
    check(fresh.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.raw_attribute("label")? == Some(json!("原对象草稿")))
}
