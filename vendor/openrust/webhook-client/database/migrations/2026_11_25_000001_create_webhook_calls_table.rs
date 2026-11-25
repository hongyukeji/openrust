use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateWebhookCallsTable;
pub(crate) static MIGRATION: CreateWebhookCallsTable = CreateWebhookCallsTable;
impl Migration for CreateWebhookCallsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_calls", |table| {
                    table.string("id", 36).primary();
                    table.string("scope", 128);
                    table.string("scope_key", 64).index();
                    table.string("name", 128);
                    table.string("revision", 128);
                    table.text("url");
                    table.string("method", 32);
                    table.long_text("headers");
                    table.long_text("raw_body");
                    table.big_integer("created_at");
                    table.text("exception").nullable();
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
                .drop_if_exists("webhook_calls")
                .await
        })
    }
}
