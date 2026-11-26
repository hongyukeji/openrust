use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddProcessingRecoveryToWebhookDispatchIntents;
pub(crate) static MIGRATION: AddProcessingRecoveryToWebhookDispatchIntents =
    AddProcessingRecoveryToWebhookDispatchIntents;

impl Migration for AddProcessingRecoveryToWebhookDispatchIntents {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_dispatch_intents", |table| {
                    table.integer("processing_retry_after").nullable();
                    table.integer("processing_requeues_left").default(0);
                    table.big_integer("enqueued_at").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_dispatch_intents")
                && query
                    .where_not_null("processing_retry_after")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 仍有启用处理恢复的接收记录",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_dispatch_intents", |table| {
                    table.drop_column(&[
                        "processing_retry_after",
                        "processing_requeues_left",
                        "enqueued_at",
                    ]);
                })
                .await
        })
    }
}
