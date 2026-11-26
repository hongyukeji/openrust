use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddExpiryToWebhookEventReceiptsTable;
pub(crate) static MIGRATION: AddExpiryToWebhookEventReceiptsTable =
    AddExpiryToWebhookEventReceiptsTable;

impl Migration for AddExpiryToWebhookEventReceiptsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_event_receipts", |table| {
                    table.big_integer("expires_at").nullable();
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
                .table("webhook_event_receipts", |table| {
                    table.drop_column(&["expires_at"]);
                })
                .await
        })
    }
}
