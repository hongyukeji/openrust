use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddProcessingLeaseToWebhookCallsTable;
pub(crate) static MIGRATION: AddProcessingLeaseToWebhookCallsTable =
    AddProcessingLeaseToWebhookCallsTable;

impl Migration for AddProcessingLeaseToWebhookCallsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.string("processing_token", 36).nullable();
                    table.big_integer("processing_generation").default(0_i64);
                    table.big_integer("processing_until").nullable();
                    table.big_integer("processed_at").nullable();
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
                        "processing_token",
                        "processing_generation",
                        "processing_until",
                        "processed_at",
                    ]);
                })
                .await
        })
    }
}
