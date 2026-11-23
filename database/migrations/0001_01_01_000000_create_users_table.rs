use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
use openrust::translation::__;

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.get_schema_builder();
            schema
                .create("users", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.users", &Default::default())?);
                    table
                        .id()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .string("name", None)
                        .comment(__("schema.columns.username", &Default::default())?);
                    table
                        .string("email", None)
                        .unique()
                        .comment(__("schema.columns.email", &Default::default())?);
                    table
                        .timestamp("email_verified_at", 0)
                        .nullable()
                        .comment(__("schema.columns.email_verified_at", &Default::default())?);
                    table
                        .string("password", None)
                        .comment(__("schema.columns.password", &Default::default())?);
                    table
                        .remember_token()
                        .comment(__("schema.columns.remember_token", &Default::default())?);
                    table
                        .timestamp("created_at", None)
                        .nullable()
                        .comment(__("schema.columns.created_at", &Default::default())?);
                    table
                        .timestamp("updated_at", None)
                        .nullable()
                        .comment(__("schema.columns.updated_at", &Default::default())?);
                    Ok(())
                })
                .await?;
            schema
                .create("password_reset_tokens", |table| -> DatabaseResult<()> {
                    table.comment(__(
                        "schema.tables.password_reset_tokens",
                        &Default::default(),
                    )?);
                    table
                        .string("email", None)
                        .primary()
                        .comment(__("schema.columns.email", &Default::default())?);
                    table
                        .string("token", None)
                        .comment(__("schema.columns.token", &Default::default())?);
                    table
                        .timestamp("created_at", 0)
                        .nullable()
                        .comment(__("schema.columns.created_at", &Default::default())?);
                    Ok(())
                })
                .await?;
            schema
                .create("sessions", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.sessions", &Default::default())?);
                    table
                        .string("id", None)
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .foreign_id("user_id")
                        .nullable()
                        .index()
                        .comment(__("schema.columns.user_id", &Default::default())?);
                    table
                        .string("ip_address", 45)
                        .nullable()
                        .comment(__("schema.columns.ip_address", &Default::default())?);
                    table
                        .text("user_agent")
                        .nullable()
                        .comment(__("schema.columns.user_agent", &Default::default())?);
                    table
                        .long_text("payload")
                        .comment(__("schema.columns.session_data", &Default::default())?);
                    table
                        .integer("last_activity")
                        .index()
                        .comment(__("schema.columns.last_activity", &Default::default())?);
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
            let mut schema = connection.get_schema_builder();
            schema.drop_if_exists("users").await?;
            schema.drop_if_exists("password_reset_tokens").await?;
            schema.drop_if_exists("sessions").await
        })
    }
}
