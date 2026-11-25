use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct ExtendWebhookDeliveriesForRecovery;
pub(crate) static MIGRATION: ExtendWebhookDeliveriesForRecovery =
    ExtendWebhookDeliveriesForRecovery;

impl Migration for ExtendWebhookDeliveriesForRecovery {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.big_integer("next_wake_at").nullable();
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
                .table("webhook_deliveries", |table| {
                    table.drop_column(&["next_wake_at"]);
                })
                .await
        })
    }
}
