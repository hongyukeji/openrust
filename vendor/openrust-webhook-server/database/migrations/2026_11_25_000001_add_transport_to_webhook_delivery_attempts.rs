use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookDeliveryAttemptTransport;
pub(crate) static MIGRATION: AddWebhookDeliveryAttemptTransport =
    AddWebhookDeliveryAttemptTransport;

impl Migration for AddWebhookDeliveryAttemptTransport {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_delivery_attempts", |table| {
                    table.big_integer("transfer_time_micros").nullable();
                    table.integer("transport_failure").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_delivery_attempts")
                && query
                    .where_not_null("transfer_time_micros")
                    .or_where_not_null("transport_failure")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 投递仍有传输诊断"));
            }
            context
                .get_schema_builder()
                .table("webhook_delivery_attempts", |table| {
                    table.drop_column(&["transfer_time_micros", "transport_failure"]);
                })
                .await
        })
    }
}
