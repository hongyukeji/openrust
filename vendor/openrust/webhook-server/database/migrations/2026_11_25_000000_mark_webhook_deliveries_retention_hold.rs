use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookDeliveriesRetentionHold;
pub(crate) static MIGRATION: AddWebhookDeliveriesRetentionHold = AddWebhookDeliveriesRetentionHold;

impl Migration for AddWebhookDeliveriesRetentionHold {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.big_integer("retention_hold_at").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_deliveries")
                && query.where_not_null("retention_hold_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 记录仍处于保留状态",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.drop_column(&["retention_hold_at"]);
                })
                .await
        })
    }
}
