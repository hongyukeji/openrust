use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddDeferredProcessingToWebhookCallsTable;
pub(crate) static MIGRATION: AddDeferredProcessingToWebhookCallsTable =
    AddDeferredProcessingToWebhookCallsTable;

impl Migration for AddDeferredProcessingToWebhookCallsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.big_integer("processing_next_at").nullable();
                    table.big_integer("processing_deadline").nullable();
                    table.big_integer("processing_deferrals").default(0_i64);
                    table.big_integer("processing_stopped_at").nullable();
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
                .table("webhook_calls", |table| {
                    table.drop_column(&[
                        "processing_next_at",
                        "processing_deadline",
                        "processing_deferrals",
                        "processing_stopped_at",
                    ]);
                })
                .await
        })
    }
}
