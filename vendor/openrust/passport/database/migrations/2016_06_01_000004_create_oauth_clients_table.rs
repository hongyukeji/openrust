use openrust::translation::__;
// Translated from the framework Passport v13.10.0 (MIT, Taylor Otwell).
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
                .create("oauth_clients", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.oauth_clients", &Default::default())?);
                    table
                        .uuid("id")
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table.nullableMorphs("owner", None, None);
                    table
                        .string("name", None)
                        .comment(__("schema.columns.name", &Default::default())?);
                    table
                        .string("secret", None)
                        .nullable()
                        .comment(__("schema.columns.secret", &Default::default())?);
                    table
                        .string("provider", None)
                        .nullable()
                        .comment(__("schema.columns.provider", &Default::default())?);
                    table
                        .text("redirect_uris")
                        .comment(__("schema.columns.redirect_uris", &Default::default())?);
                    table
                        .text("grant_types")
                        .comment(__("schema.columns.grant_types", &Default::default())?);
                    table
                        .boolean("revoked")
                        .comment(__("schema.columns.revoked", &Default::default())?);
                    table
                        .timestamp("created_at", None)
                        .nullable()
                        .comment(__("schema.columns.created_at", &Default::default())?);
                    table
                        .timestamp("updated_at", None)
                        .nullable()
                        .comment(__("schema.columns.updated_at", &Default::default())?);

                    for column in table.getColumnsMut() {
                        match column.getName() {
                            "owner_type" => {
                                column
                                    .comment(__("schema.columns.owner_type", &Default::default())?);
                            }
                            "owner_id" => {
                                column.comment(__("schema.columns.owner_id", &Default::default())?);
                            }
                            _ => {}
                        }
                    }
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
                .dropIfExists("oauth_clients")
                .await
        })
    }
}
