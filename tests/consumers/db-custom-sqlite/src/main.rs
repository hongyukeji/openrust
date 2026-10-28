use figment::Figment;
use openrust::config::FrameworkConfig;
use openrust::core::{App, AppBuilder, BoxFuture, Error, Plugin, Result};
use openrust::db::{
    DatabaseTableNames, Db, DbBackend, MigrationPlugin, sea_orm, sea_orm_migration,
};
use openrust::lifecycle::finalize;
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

struct CustomSqlitePlugin;

impl Plugin for CustomSqlitePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        app.try_insert(Db::default())?;
        Ok(())
    }

    fn finish<'a>(&'a self, app: &'a App) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let connection = Database::connect("sqlite::memory:")
                .await
                .map_err(Error::internal)?;
            app.state().try_get::<Db>()?.initialize(connection)
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut builder = AppBuilder::new(FrameworkConfig::default(), Figment::new());
    builder
        .plugin(CustomSqlitePlugin)
        .plugin(MigrationPlugin::<Migrator>::new());
    let app = finalize(builder).await?;
    let db = app.state().try_get::<Db>()?;
    assert_eq!(db.backend()?, DbBackend::Sqlite);
    assert_eq!(db.quoted_table_name("entries")?, "\"tb_entries\"");
    let unsupported_migration = app
        .run_command("migrate", vec!["up".into()])
        .await
        .err()
        .ok_or_else(|| {
            Error::custom(
                "expected_unsupported_backend",
                "SQLite migration lock accepted",
            )
        })?;
    assert_eq!(unsupported_migration.code(), "unsupported_database_backend");

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
    println!("自定义 SQLite 插件连接、提交与回滚验证通过");
    Ok(())
}
