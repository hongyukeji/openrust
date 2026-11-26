use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct MarkWebhookMessagesIdempotencyExpiry;
pub(crate) static MIGRATION: MarkWebhookMessagesIdempotencyExpiry =
    MarkWebhookMessagesIdempotencyExpiry;

impl Migration for MarkWebhookMessagesIdempotencyExpiry {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_messages", |table| {
                    table.big_integer("idempotency_expired_at").nullable();
                    table.index(
                        &[
                            "scope_key",
                            "idempotency_expired_at",
                            "body_pruned_at",
                            "id",
                        ],
                        Some("webhook_messages_key_expiry"),
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
            if let Some(query) = context.query_table("webhook_messages")
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
                .table("webhook_messages", |table| {
                    table.drop_index("webhook_messages_key_expiry");
                    table.drop_column(&["idempotency_expired_at"]);
                })
                .await
        })
    }
}
