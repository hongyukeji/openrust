use figment::Figment;
use openrust::core::Error;
use openrust::core::Result;
use openrust::database::{
    DatabaseTableNames, DbBackend, MigrationServiceProvider, PageContext, SortDirection, sea_orm,
    sea_orm_migration, with_page_context,
};
use openrust::foundation::Application;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::RegisterContext;
use openrust::prelude::Db;
use openrust::support::ServiceProvider;
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::prelude::*;

struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(CreateProbe)]
    }
}

struct CreateProbe;

impl MigrationName for CreateProbe {
    fn name(&self) -> &'static str {
        "m20261026_000001_create_plugin_probe"
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl MigrationTrait for CreateProbe {
    async fn up(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let names = DatabaseTableNames::current()
            .ok_or_else(|| DbErr::Migration("missing table-name scope".to_owned()))?;
        let table = names
            .resolve("plugin_probes")
            .map_err(|error| DbErr::Migration(error.to_string()))?;
        manager
            .create_table(
                Table::create()
                    .table(Alias::new(table))
                    .if_not_exists()
                    .col(ColumnDef::new(Probe::Id).integer().not_null().primary_key())
                    .col(ColumnDef::new(Probe::Value).string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> std::result::Result<(), DbErr> {
        let names = DatabaseTableNames::current()
            .ok_or_else(|| DbErr::Migration("missing table-name scope".to_owned()))?;
        let table = names
            .resolve("plugin_probes")
            .map_err(|error| DbErr::Migration(error.to_string()))?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new(table))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Probe {
    Id,
    Value,
}

struct CustomSqliteProvider;
impl ServiceProvider for CustomSqliteProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<Db, _>(|resolver| {
                Box::pin(async move {
                    let connection = Database::connect("sqlite::memory:")
                        .await
                        .map_err(Error::internal)?;
                    let closing = connection.clone();
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(
                            async move { closing.close_by_ref().await.map_err(Error::internal) },
                        )
                    })?;
                    let db = Db::with_table_names(DatabaseTableNames::new(
                        "tb_",
                        std::collections::BTreeMap::new(),
                    )?);
                    db.initialize(connection)?;
                    Ok(std::sync::Arc::new(db))
                })
            })
            .await?;
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(CustomSqliteProvider)
        .provider(MigrationServiceProvider::<Migrator>::new());
    let app = Application::from_provider_runtime(builder.start(Figment::new()).await?)?;
    let db = app.state().resolve::<Db>().await?;
    assert_eq!(db.backend()?, DbBackend::Sqlite);
    assert_eq!(db.quoted_table_name("entries")?, "\"tb_entries\"");
    let connection = db.connection()?;
    // 框架迁移互斥限于双库；此自定义后端由应用自行执行单机迁移。
    db.table_names()
        .scope(Migrator::up(&connection, None))
        .await
        .map_err(Error::internal)?;
    let migrated_probe = connection
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM \"tb_plugin_probes\"".to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("missing_migration_probe", "migration probe missing"))?;
    assert_eq!(
        migrated_probe
            .try_get::<i64>("", "count")
            .map_err(Error::internal)?,
        0
    );
    let duplicate_initialization = db
        .initialize(connection.clone())
        .err()
        .ok_or_else(|| Error::custom("expected_duplicate_database", "Db initialized twice"))?;
    assert_eq!(
        duplicate_initialization.code(),
        "database_already_initialized"
    );
    connection
        .execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE \"tb_entries\" (value TEXT NOT NULL)".to_owned(),
        ))
        .await
        .map_err(Error::internal)?;

    db.transaction(|transaction| {
        Box::pin(async move {
            transaction
                .execute_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    "INSERT INTO \"tb_entries\" (value) VALUES ('committed')".to_owned(),
                ))
                .await
                .map_err(Error::internal)?;
            Ok(())
        })
    })
    .await?;

    let rollback = db
        .transaction::<(), _>(|transaction| {
            Box::pin(async move {
                transaction
                    .execute_raw(Statement::from_string(
                        DbBackend::Sqlite,
                        "INSERT INTO \"tb_entries\" (value) VALUES ('rolled back')".to_owned(),
                    ))
                    .await
                    .map_err(Error::internal)?;
                Err(Error::custom("consumer_rejected", "rollback requested"))
            })
        })
        .await;
    assert!(rollback.is_err());

    let count = connection
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM \"tb_entries\"".to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("missing_row", "count query returned no row"))?
        .try_get::<i64>("", "count")
        .map_err(Error::internal)?;
    assert_eq!(count, 1);
    let entries = db.table("entries")?;
    assert_eq!(entries.count().await?, 1);
    assert_eq!(
        entries
            .clone()
            .where_eq("value", "committed")?
            .value::<String>("value")
            .await?
            .as_deref(),
        Some("committed")
    );
    assert_eq!(
        entries.insert(vec![("value", "via-query".into())]).await?,
        1
    );
    let page_context = PageContext::from_uri(&"/entries?page=2".parse().map_err(Error::internal)?);
    let page = with_page_context(
        page_context,
        entries
            .clone()
            .order_by("value", SortDirection::Asc)?
            .paginate(1),
    )
    .await?;
    assert_eq!(
        (
            page.current_page,
            page.total,
            page.last_page,
            page.items.len()
        ),
        (2, 2, 2, 1)
    );
    assert_eq!(page.previous_page_url().as_deref(), Some("/entries?page=1"));
    assert_eq!(
        entries
            .clone()
            .where_eq("value", "via-query")?
            .update(vec![("value", "updated".into())])
            .await?,
        1
    );
    assert_eq!(
        entries
            .clone()
            .where_eq("value", "updated")?
            .delete()
            .await?,
        1
    );
    assert_eq!(entries.count().await?, 1);
    let unsupported_migration = app
        .run_command("migrate", vec!["up".into()])
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "expected_unsupported_backend",
                "未启用内置 SQLite 时迁移命令必须拒绝",
            )
        })?;
    // 自定义 SQLx 连接仍可事务/查询；框架 SQLite 互斥须显式编译 db-sqlite。
    assert_eq!(unsupported_migration.code(), "database_feature_disabled");

    println!("自定义 SQLite Provider 连接、提交与回滚验证通过");
    Ok(())
}
