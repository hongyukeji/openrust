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
                .virtual_as("unhex(md5(`key`))");
        }
        "pgsql" => {
            table.uuid("key_hash").stored_as("md5(\"key\")::uuid");
        }
        _ => {
            table.string("key_hash", None);
        }
    }
}
impl Migration for CreatePulseTables {
    fn should_run<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async move {
            if matches!(
                context.get_driver_name(),
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

    fn get_connection(
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
            if !self.should_run(context).await? {
                return Ok(());
            }
            let driver = context.get_driver_name();
            let mut schema = context.get_schema_builder();
            schema
                .create("pulse_values", |t| {
                    t.id();
                    t.unsigned_integer("timestamp");
                    t.string("type", None);
                    t.medium_text("key");
                    hash(t, driver);
                    t.medium_text("value");
                    t.index(&["timestamp"], None);
                    t.index(&["type"], None);
                    t.unique(&["type", "key_hash"], None);
                })
                .await?;
            schema
                .create("pulse_entries", |t| {
                    t.id();
                    t.unsigned_integer("timestamp");
                    t.string("type", None);
                    t.medium_text("key");
                    hash(t, driver);
                    t.big_integer("value").nullable();
                    t.index(&["timestamp"], None);
                    t.index(&["type"], None);
                    t.index(&["key_hash"], None);
                    t.index(&["timestamp", "type", "key_hash", "value"], None);
                })
                .await?;
            schema
                .create("pulse_aggregates", |t| {
                    t.id();
                    t.unsigned_integer("bucket");
                    t.unsigned_medium_integer("period");
                    t.string("type", None);
                    t.medium_text("key");
                    hash(t, driver);
                    t.string("aggregate", None);
                    t.decimal("value", 20, 2);
                    t.unsigned_integer("count").nullable();
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
            let mut schema = context.get_schema_builder();
            for table in ["pulse_values", "pulse_entries", "pulse_aggregates"] {
                schema.drop_if_exists(table).await?;
            }
            Ok(())
        })
    }
}
