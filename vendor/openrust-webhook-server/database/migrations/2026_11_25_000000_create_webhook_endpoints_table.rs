use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookEndpoints;
pub(crate) static MIGRATION: CreateWebhookEndpoints = CreateWebhookEndpoints;

impl Migration for CreateWebhookEndpoints {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_endpoints", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("name", 128);
                    table.integer("revision");
                    table.string("state", 16);
                    table.text("url");
                    table.string("credential", 128).nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_endpoints")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 端点与控制状态仍存在",
                ));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_endpoints")
                .await
        })
    }
}
