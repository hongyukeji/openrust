use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct ExtendWebhookDeliveriesForRetention;
pub(crate) static MIGRATION: ExtendWebhookDeliveriesForRetention =
    ExtendWebhookDeliveriesForRetention;

impl Migration for ExtendWebhookDeliveriesForRetention {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.big_integer("payload_pruned_at").nullable();
                    table.index(
                        &["scope_key", "deadline", "id"],
                        Some("webhook_delivery_retention"),
                    );
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
                && query.where_not_null("payload_pruned_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 正文墓碑仍存在"));
            }
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.drop_index("webhook_delivery_retention");
                    table.drop_column(&["payload_pruned_at"]);
                })
                .await
        })
    }
}
