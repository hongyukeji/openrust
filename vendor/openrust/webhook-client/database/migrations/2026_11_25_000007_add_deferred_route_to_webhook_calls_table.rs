use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddDeferredRouteToWebhookCallsTable;
pub(crate) static MIGRATION: AddDeferredRouteToWebhookCallsTable =
    AddDeferredRouteToWebhookCallsTable;

impl Migration for AddDeferredRouteToWebhookCallsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.text("processing_deferred_route").nullable();
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
                    table.drop_column(&["processing_deferred_route"]);
                })
                .await
        })
    }
}
