use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
    schema::Blueprint,
};
pub struct CreatePulseTables;
fn hash(table: &mut Blueprint, driver: &str) {
    match driver {
        "mysql" | "mariadb" => {
            table
                .char("key_hash", 16)
                .charset("binary")
                .virtualAs("unhex(md5(`key`))");
        }
        "pgsql" => {
            table.uuid("key_hash").storedAs("md5(\"key\")::uuid");
        }
        _ => {
            table.string("key_hash", None);
        }
    }
}
impl Migration for CreatePulseTables {
    fn shouldRun<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            if matches!(
                context.getDriverName(),
                "mysql" | "mariadb" | "pgsql" | "sqlite"
            ) {
                return Ok(true);
            }
            use openrust::support::facades::{Config, Facade};
            let config = Config::get_facade_root()
                .map_err(openrust::database::DatabaseError::Unavailable)?;
            if config
                .get("app.env")
                .and_then(|v| v.as_str().map(str::to_owned))
                .as_deref()
                == Some("testing")
                && config.get("pulse.enabled").and_then(|v| v.as_bool()) == Some(false)
            {
                return Ok(false);
            }
            Err(openrust::database::DatabaseError::InvalidMigration(
                "Pulse supports only MySQL, MariaDB, PostgreSQL and SQLite; disable Pulse in tests using an unsupported driver",
            ))
        })
    }

    fn getConnection(
        &self,
        config: &dyn openrust::contracts::config::Repository,
    ) -> DatabaseResult<Option<String>> {
        match config.get("pulse.storage.database.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) => Ok(Some(name)),
            _ => Err(openrust::database::DatabaseError::InvalidQuery(
                "Pulse migration connection",
            )),
        }
    }

    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if !self.shouldRun(context).await? {
                return Ok(());
            }
            let driver = context.getDriverName();
            let mut schema = context.getSchemaBuilder();
            schema
                .create("pulse_values", |t| {
                    t.id();
                    t.unsignedInteger("timestamp");
                    t.string("type", None);
                    t.mediumText("key");
                    hash(t, driver);
                    t.mediumText("value");
                    t.index(&["timestamp"], None);
                    t.index(&["type"], None);
                    t.unique(&["type", "key_hash"], None);
                })
                .await?;
            schema
                .create("pulse_entries", |t| {
                    t.id();
                    t.unsignedInteger("timestamp");
                    t.string("type", None);
                    t.mediumText("key");
                    hash(t, driver);
                    t.bigInteger("value").nullable();
                    t.index(&["timestamp"], None);
                    t.index(&["type"], None);
                    t.index(&["key_hash"], None);
                    t.index(&["timestamp", "type", "key_hash", "value"], None);
                })
                .await?;
            schema
                .create("pulse_aggregates", |t| {
                    t.id();
                    t.unsignedInteger("bucket");
                    t.unsignedMediumInteger("period");
                    t.string("type", None);
                    t.mediumText("key");
                    hash(t, driver);
                    t.string("aggregate", None);
                    t.decimal("value", 20, 2);
                    t.unsignedInteger("count").nullable();
                    t.unique(&["bucket", "period", "type", "aggregate", "key_hash"], None);
                    t.index(&["period", "bucket"], None);
                    t.index(&["type"], None);
                    t.index(&["period", "type", "aggregate", "bucket"], None);
                })
                .await?;
            Ok(())
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.getSchemaBuilder();
            for table in ["pulse_values", "pulse_entries", "pulse_aggregates"] {
                schema.dropIfExists(table).await?;
            }
            Ok(())
        })
    }
}
