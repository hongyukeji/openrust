use openrust::localization::__;
// Translated from the framework Passport v13.10.0 (MIT, Taylor Otwell).
use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct Definition;

impl Migration for Definition {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
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
                .get_schema_builder()
                .create("oauth_device_codes", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.oauth_device_codes", &Default::default())?);
                    table
                        .char("id", 80)
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .foreign_id("user_id")
                        .nullable()
                        .index()
                        .comment(__("schema.columns.user_id", &Default::default())?);
                    table
                        .foreign_uuid("client_id")
                        .index()
                        .comment(__("schema.columns.client_id", &Default::default())?);
                    table
                        .char("user_code", 8)
                        .unique()
                        .comment(__("schema.columns.user_code", &Default::default())?);
                    table
                        .text("scopes")
                        .comment(__("schema.columns.scopes", &Default::default())?);
                    table
                        .boolean("revoked")
                        .comment(__("schema.columns.revoked", &Default::default())?);
                    table
                        .date_time("user_approved_at", None)
                        .nullable()
                        .comment(__("schema.columns.user_approved_at", &Default::default())?);
                    table
                        .date_time("last_polled_at", None)
                        .nullable()
                        .comment(__("schema.columns.last_polled_at", &Default::default())?);
                    table
                        .date_time("expires_at", None)
                        .nullable()
                        .comment(__("schema.columns.expires_at", &Default::default())?);
                    Ok(())
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
                .get_schema_builder()
                .drop_if_exists("oauth_device_codes")
                .await
        })
    }
}
