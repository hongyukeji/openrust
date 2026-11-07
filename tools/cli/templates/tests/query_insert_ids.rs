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
    relations: openrust::database::eloquent::ModelRelations,
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
            "id" | "owner_id" => value.as_i64().map(Value::from),
            "label" => value.as_str().map(Value::from),
            _ => None,
        }
        .ok_or_else(|| Error::custom("generated_model_binding", "生成应用模型列类型错误"))
    }
}

impl openrust::database::eloquent::HydrateModel for GeneratedModel {
    fn hydrate(
        &self,
        raw: serde_json::Map<String, serde_json::Value>,
        connection: Option<&str>,
    ) -> Result<Self> {
        let mut metadata = self.metadata.clone();
        metadata.connection = connection.map(str::to_owned);
        Ok(Self {
            metadata,
            attributes: openrust::database::eloquent::ModelAttributes::new(raw),
            relations: Default::default(),
            exists: true,
        })
    }

    fn load_relation<'a>(
        models: &'a mut [Self],
        relation: openrust::database::eloquent::RelationLoad<'a>,
        context: &'a openrust::database::eloquent::SaveContext<'_>,
        transaction: Option<&'a openrust::database::TransactionContext<'_>>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if relation.name != "items" {
                return Err(Error::custom(
                    "generated_relation_unavailable",
                    "生成夹具未声明该关系加载",
                ));
            }
            let Some(model) = models.first() else {
                return Ok(());
            };
            let mut related = model.hydrate(Default::default(), None)?;
            related.metadata.table = "relation_items".into();
            openrust::database::eloquent::HasMany::new(&related, "owner_id", "id")
                .load_with(models, relation, context, transaction, Ok)
                .await
        })
    }
}

impl openrust::database::eloquent::HasRelationships for GeneratedModel {
    fn relations(&self) -> &openrust::database::eloquent::ModelRelations {
        &self.relations
    }

    fn relations_mut(&mut self) -> &mut openrust::database::eloquent::ModelRelations {
        &mut self.relations
    }
}

async fn check_generated_has_many(
    db: &Db,
    context: &openrust::database::eloquent::SaveContext<'_>,
) -> Result<()> {
    use openrust::database::{
        SortDirection,
        eloquent::{HasMany, ModelAttributes, ModelMetadata},
    };
    use serde_json::{Map, json};

    let connection = db.connection()?;
    connection
        .execute_unprepared("CREATE TABLE relation_items (id BIGINT PRIMARY KEY, owner_id BIGINT, label VARCHAR(100))")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("INSERT INTO relation_items (id, owner_id, label) VALUES (1, 7, 'a'), (2, 7, 'b'), (3, 8, 'c'), (4, 20, 'other'), (5, NULL, 'none')")
        .await
        .map_err(Error::internal)?;
    let make_model = |table, class_name, key: serde_json::Value| GeneratedModel {
        metadata: ModelMetadata::new(table, class_name),
        attributes: ModelAttributes::new(Map::from_iter([("id".into(), key)])),
        relations: Default::default(),
        exists: true,
    };
    let related = make_model("relation_items", "generated::InsertItem", json!(null));
    let mut parents = [json!(7), json!(8), json!(7), json!(9), json!(null)]
        .map(|key| make_model("insert_items", "generated::Owner", key));
    for parent in &mut parents {
        parent.metadata.connection = Some("other".into());
    }
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    let has_many = HasMany::new(&related, "owner_id", "id");
    has_many
        .load(&mut parents, "items", context, move |query| {
            query.order_by("id", SortDirection::Desc).map(|query| {
                query.before_query(move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
        })
        .await?;
    check(observed.load(Ordering::SeqCst) == 1)?;
    for (parent, expected) in parents.iter().zip([&[2, 1][..], &[3], &[2, 1], &[], &[]]) {
        let children = parent
            .relations
            .get_relation("items")
            .and_then(|value| value.downcast_ref::<Vec<Arc<GeneratedModel>>>())
            .ok_or_else(|| Error::custom("generated_has_many_unloaded", "关联集合未加载"))?;
        check(children.len() == expected.len())?;
        for (child, id) in children.iter().zip(expected) {
            check(child.exists && child.attributes.raw_attribute("id")? == Some(json!(id)))?;
            check(child.metadata.connection.as_deref() == Some("other"))?;
            check(child.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
        }
    }
    Ok(())
}

async fn generated_model_save(url: &str) -> Result<()> {
    use openrust::database::{
        DatabaseServiceProvider,
        eloquent::{MassAssignment, ModelMetadata, SaveContext, save, save_or_fail},
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
            relations: Default::default(),
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
        check_generated_has_many(&db, &context).await?;
        check_generated_model_readback(
            databases.clone(),
            events.clone(),
            &context,
            model,
            original,
            replacement,
        )
        .await
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

async fn check_generated_model_readback(
    databases: Arc<openrust::Databases>,
    events: Arc<openrust::events::Events>,
    context: &openrust::database::eloquent::SaveContext<'_>,
    mut model: GeneratedModel,
    original: i64,
    replacement: i64,
) -> Result<()> {
    use openrust::database::eloquent::refresh;
    use serde_json::{Map, json};

    let db = databases.default_database();
    let changes = model.attributes.get_changes()?;
    model
        .attributes
        .set_raw_attribute("label", json!("内存草稿"))?;
    db.table("insert_items")?
        .where_eq("id", replacement)?
        .update(vec![("label", "外部更新".into())])
        .await?;
    refresh(&mut model, context).await?;
    check(model.attributes.get_raw_original("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.is_clean(&model.metadata, &[])?)?;
    check(model.attributes.get_changes()? == changes)?;
    check_model_relations(&mut model)?;
    check_fresh_model(&model, context).await?;
    check_failed_refresh(&db, &mut model, context, replacement).await?;
    model.relations.unset_relation("owner");
    check_readback_relations(&db, &mut model, context, replacement).await?;
    let mut absent = GeneratedModel {
        metadata: model.metadata.clone(),
        attributes: openrust::database::eloquent::ModelAttributes::new(Map::from_iter([
            ("id".into(), json!(original)),
            ("label".into(), json!("原基线")),
        ])),
        relations: Default::default(),
        exists: true,
    };
    absent
        .attributes
        .set_raw_attribute("label", json!("本地草稿"))?;
    generated_locked_model_refresh(databases, events, model).await?;
    let missing = refresh(&mut absent, context).await;
    check(matches!(missing, Err(error) if error.code() == "model_not_found"))?;
    check(absent.attributes.get_raw_original("label")? == Some(json!("原基线")))?;
    check(absent.attributes.raw_attribute("label")? == Some(json!("本地草稿")))
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
    let result = db.transaction_for_update_checked_context(
        move |transaction| {
            Box::pin(async move {
                let calendar = CalendarContext::default();
                let context = SaveContext::new(&databases, Some(&events), &calendar);
                model.metadata.connection = Some("other".into());
                let wrong_source = refresh_for_update(&mut model, &context, &transaction).await;
                check(matches!(wrong_source, Err(error)
                    if error.code() == "model_refresh_transaction_source_mismatch"))?;
                check(model.attributes.raw_attribute("label")? == Some(json!("锁定前草稿")))?;
                let wrong_relation = <GeneratedModel as openrust::database::eloquent::HydrateModel>::load_relation(
                    std::slice::from_mut(&mut model),
                    openrust::database::eloquent::RelationLoad { name: "items", nested: &[] },
                    &context,
                    Some(&transaction),
                ).await;
                check(matches!(wrong_relation, Err(error)
                    if error.code() == "model_relation_transaction_source_mismatch"))?;
                model.metadata.connection = None;
                let id = model.attributes.raw_attribute("id")?.and_then(|id| id.as_i64())
                    .ok_or_else(|| Error::custom("generated_locked_key_missing", "锁定回读缺少主键"))?;
                databases.default_database().table("relation_items")?
                    .using_transaction(transaction.transaction())
                    .insert(vec![("id", 99_i64.into()), ("owner_id", id.into()), ("label", "未提交子行".into())])
                    .await?;
                refresh_for_update(&mut model, &context, &transaction).await?;
                check_locked_child(&model)?;
                check(
                    model.attributes.get_raw_original("label")? == Some(json!("事务内监听器读取")),
                )?;
                check(model.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))?;
                check(model.attributes.is_clean(&model.metadata, &[])?)?;
                Err::<(), _>(Error::custom("generated_relation_rollback", "验证关系回读后回滚"))
            })
        },
        |_| Box::pin(async { Ok(()) }),
    )
    .await;
    check(matches!(result, Err(error) if error.code() == "generated_relation_rollback"))?;
    check(
        db.table("relation_items")?
            .where_eq("id", 99_i64)?
            .count()
            .await?
            == 0,
    )
}

fn check_locked_child(model: &GeneratedModel) -> Result<()> {
    use serde_json::json;

    let children = model
        .relations
        .get_relation("items")
        .and_then(|relation| relation.downcast_ref::<Vec<Arc<GeneratedModel>>>())
        .ok_or_else(|| Error::custom("generated_locked_relation_missing", "锁定回读未加载关系"))?;
    check(children.len() == 2)?;
    let child = children
        .iter()
        .find(|child| {
            child
                .attributes
                .raw_attribute("id")
                .is_ok_and(|id| id == Some(json!(99)))
        })
        .ok_or_else(|| {
            Error::custom("generated_uncommitted_child_missing", "未读到事务中的子行")
        })?;
    check(child.attributes.get_raw_original("label")? == Some(json!("未提交子行")))?;
    check(child.attributes.raw_attribute("label")? == Some(json!("事务内监听器读取")))
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

async fn check_readback_relations(
    db: &Db,
    model: &mut GeneratedModel,
    context: &openrust::database::eloquent::SaveContext<'_>,
    key: i64,
) -> Result<()> {
    use openrust::database::eloquent::{LoadedRelation, fresh_with, refresh};
    use serde_json::json;

    db.table("relation_items")?
        .where_eq("id", 1_i64)?
        .update(vec![("owner_id", key.into())])
        .await?;
    db.table("relation_items")?
        .insert(vec![
            ("id", 11_i64.into()),
            ("owner_id", 1_i64.into()),
            ("label", "孙行".into()),
        ])
        .await?;
    let fresh = fresh_with(model, context, &["items", "items.items", "items.items"])
        .await?
        .ok_or_else(|| Error::custom("generated_related_fresh_missing", "回读未找到模型"))?;
    check_nested_items(&fresh)?;
    check(!model.relations.relation_loaded("items"))?;

    model.relations.set_relation(
        "items",
        LoadedRelation::Related(Box::new(Vec::<Arc<GeneratedModel>>::new())),
    );
    refresh(model, context).await?;
    check(
        model
            .relations
            .get_relation("items")
            .and_then(LoadedRelation::downcast_ref::<Vec<Arc<GeneratedModel>>>)
            .is_some_and(|items| items.len() == 1),
    )?;
    check(model.relations.relation_loaded("membership"))?;
    model
        .attributes
        .set_raw_attribute("label", json!("旧基线"))?;
    model.attributes.sync_original()?;
    model
        .relations
        .set_relation("unknown", LoadedRelation::Null);
    let failed = refresh(model, context).await;
    check(matches!(failed, Err(error) if error.code() == "generated_relation_unavailable"))?;
    check(model.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.get_raw_original("label")? == Some(json!("旧基线")))?;
    model.relations.unset_relation("unknown");
    Ok(())
}

fn check_nested_items(model: &GeneratedModel) -> Result<()> {
    use serde_json::json;

    let items = |model: &GeneratedModel| -> Result<Vec<Arc<GeneratedModel>>> {
        model
            .relations
            .get_relation("items")
            .and_then(|value| value.downcast_ref::<Vec<Arc<GeneratedModel>>>())
            .cloned()
            .ok_or_else(|| Error::custom("generated_nested_unloaded", "嵌套关系未加载"))
    };
    let children = items(model)?;
    check(children.len() == 1)?;
    let child = children
        .first()
        .ok_or_else(|| Error::custom("generated_child_missing", "缺少子模型"))?;
    let grandchildren = items(child)?;
    check(grandchildren.len() == 1)?;
    let grandchild = grandchildren
        .first()
        .ok_or_else(|| Error::custom("generated_grandchild_missing", "缺少孙模型"))?;
    check(grandchild.attributes.raw_attribute("id")? == Some(json!(11)))
}

fn check_model_relations(model: &mut GeneratedModel) -> Result<()> {
    use openrust::database::eloquent::LoadedRelation;

    let relations = &mut model.relations;
    check(!relations.relation_loaded("owner"))?;
    relations.set_relation("owner", LoadedRelation::Null);
    relations.set_relation(
        "items",
        LoadedRelation::Related(Box::new(Vec::<i64>::new())),
    );
    relations.set_relation("membership", LoadedRelation::Pivot(Box::new(7_i64)));
    check(matches!(
        relations.get_relation("owner"),
        Some(LoadedRelation::Null)
    ))?;
    check(relations.relation_loaded("items"))?;
    check(
        relations
            .get_relation("items")
            .and_then(LoadedRelation::downcast_ref::<Vec<i64>>)
            .is_some_and(Vec::is_empty),
    )?;
    check(relations.refreshable_names().eq(["owner", "items"]))?;
    relations.set_relation("owner", LoadedRelation::Related(Box::new(9_i64)));
    check(relations.refreshable_names().eq(["owner", "items"]))?;
    relations.unset_relation("items");
    check(!relations.relation_loaded("items"))?;
    check(relations.refreshable_names().eq(["owner"]))?;
    check(
        relations
            .get_relation("membership")
            .and_then(LoadedRelation::downcast_ref::<i64>)
            == Some(&7),
    )
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
    check(model.relations.relation_loaded("owner"))?;
    check(!fresh.relations.relation_loaded("owner"))?;
    check(fresh.attributes.get_raw_original("label")? == Some(json!("外部更新")))?;
    check(fresh.attributes.raw_attribute("label")? == Some(json!("监听器读取")))?;
    check(model.attributes.raw_attribute("label")? == Some(json!("原对象草稿")))
}
