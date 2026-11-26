use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddPayloadToWebhookCallsTable;
pub(crate) static MIGRATION: AddPayloadToWebhookCallsTable = AddPayloadToWebhookCallsTable;

impl Migration for AddPayloadToWebhookCallsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.long_text("payload").nullable();
                    table.long_text("attachment_media_type").nullable();
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
                    table.drop_column(&["payload", "attachment_media_type"]);
                })
                .await
        })
    }
}
