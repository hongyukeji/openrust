use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateWebhookEventReceiptsTable;
pub(crate) static MIGRATION: CreateWebhookEventReceiptsTable = CreateWebhookEventReceiptsTable;
impl Migration for CreateWebhookEventReceiptsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_event_receipts", |table| {
                    table.string("event_key", 64).primary();
                    table.string("fingerprint", 64);
                    table.string("call_id", 36).unique();
                    table
                        .foreign(&["call_id"], None)
                        .references("id")
                        .on("webhook_calls")
                        .cascade_on_delete();
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
                .drop_if_exists("webhook_event_receipts")
                .await
        })
    }
}
