use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct MarkWebhookDeliveriesIdempotencyExpiry;
pub(crate) static MIGRATION: MarkWebhookDeliveriesIdempotencyExpiry =
    MarkWebhookDeliveriesIdempotencyExpiry;

impl Migration for MarkWebhookDeliveriesIdempotencyExpiry {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.big_integer("idempotency_expired_at").nullable();
                    table.index(
                        &[
                            "scope_key",
                            "idempotency_expired_at",
                            "payload_pruned_at",
                            "id",
                        ],
                        Some("webhook_deliveries_key_expiry"),
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
                && query
                    .where_not_null("idempotency_expired_at")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 幂等键已有到期记录",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_deliveries", |table| {
                    table.drop_index("webhook_deliveries_key_expiry");
                    table.drop_column(&["idempotency_expired_at"]);
                })
                .await
        })
    }
}
