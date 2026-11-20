use openrust::translation::__;
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
                .create("oauth_auth_codes", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.oauth_auth_codes", &Default::default())?);
                    table
                        .char("id", 80)
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .foreignId("user_id")
                        .index()
                        .comment(__("schema.columns.user_id", &Default::default())?);
                    table
                        .foreignUuid("client_id")
                        .comment(__("schema.columns.client_id", &Default::default())?);
                    table
                        .text("scopes")
                        .nullable()
                        .comment(__("schema.columns.scopes", &Default::default())?);
                    table
                        .boolean("revoked")
                        .comment(__("schema.columns.revoked", &Default::default())?);
                    table
                        .dateTime("expires_at", None)
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
                .getSchemaBuilder()
                .dropIfExists("oauth_auth_codes")
                .await
        })
    }
}
