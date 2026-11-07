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

async fn generated_model_save(url: &str) -> Result<()> {
    use openrust::database::{
        DatabaseServiceProvider,
        eloquent::{MassAssignment, ModelMetadata, SaveContext, save, save_or_fail},
    };
    use openrust::support::CalendarContext;
    use serde_json::{Map, json};

    let mut builder = openrust::foundation::ProviderBuilder::new();
    builder.provider(DatabaseServiceProvider);
    let source = figment::Figment::new().merge(("database.url", url));
    let runtime = builder.start(source).await?;
    let outcome = async {
        let databases = runtime.container().resolve::<openrust::Databases>().await?;
        let db = databases.default_database();
        let calendar = CalendarContext::default();
        let context = SaveContext::new(&databases, None, &calendar);
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
        )
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}
