use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateWebhookHintsTable;
pub(crate) static MIGRATION: CreateWebhookHintsTable = CreateWebhookHintsTable;
impl Migration for CreateWebhookHintsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_hints", |table| {
                    table.string("id", 36).primary();
                    table.string("scope_key", 64).index();
                    table.string("scope", 128);
                    table.string("profile", 128);
                    table.string("revision", 128);
                    table.string("task_id", 128);
                    table.string("status", 16);
                    table.big_integer("expires_at");
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .drop_if_exists("webhook_hints")
                .await
        })
    }
}
