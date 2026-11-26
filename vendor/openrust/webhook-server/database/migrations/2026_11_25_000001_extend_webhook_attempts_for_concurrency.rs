use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookDeliveryConcurrency;
pub(crate) static MIGRATION: AddWebhookDeliveryConcurrency = AddWebhookDeliveryConcurrency;

impl Migration for AddWebhookDeliveryConcurrency {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_delivery_attempts", |table| {
                    table.string("send_scope_key", 64).nullable();
                    table.string("send_endpoint_id", 64).nullable();
                    table.big_integer("send_lease_until").nullable();
                    table.index(
                        &["send_scope_key", "finished_at", "send_lease_until"],
                        Some("webhook_attempt_scope_slot"),
                    );
                    table.index(
                        &[
                            "send_scope_key",
                            "send_endpoint_id",
                            "finished_at",
                            "send_lease_until",
                        ],
                        Some("webhook_attempt_endpoint_slot"),
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
            if let Some(query) = context.query_table("webhook_delivery_attempts")
                && query
                    .where_null("finished_at")
                    .where_not_null("send_lease_until")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 投递仍有尚未结算的在途尝试",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_delivery_attempts", |table| {
                    table.drop_index("webhook_attempt_scope_slot");
                    table.drop_index("webhook_attempt_endpoint_slot");
                    table.drop_column(&["send_scope_key", "send_endpoint_id", "send_lease_until"]);
                })
                .await
        })
    }
}
