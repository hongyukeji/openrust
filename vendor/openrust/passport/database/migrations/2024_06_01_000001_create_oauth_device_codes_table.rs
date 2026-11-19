// Translated from Laravel Passport v13.10.0 (MIT, Taylor Otwell).
use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct Definition;

#[allow(non_snake_case)]
impl Migration for Definition {
    fn getConnection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("passport.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) => Ok(Some(name)),
            Some(serde_json::Value::Bool(value)) => Ok(Some(if value { "1" } else { "" }.into())),
            Some(serde_json::Value::Number(value)) => Ok(Some(value.to_string())),
            _ => Err(DatabaseError::InvalidConfiguration("passport.connection")),
        }
    }
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .getSchemaBuilder()
                .create("oauth_device_codes", |table| {
                    table.char("id", 80).primary();
                    table.foreignId("user_id").nullable().index();
                    table.foreignUuid("client_id").index();
                    table.char("user_code", 8).unique();
                    table.text("scopes");
                    table.boolean("revoked");
                    table.dateTime("user_approved_at", None).nullable();
                    table.dateTime("last_polled_at", None).nullable();
                    table.dateTime("expires_at", None).nullable();
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .getSchemaBuilder()
                .dropIfExists("oauth_device_codes")
                .await
        })
    }
}
