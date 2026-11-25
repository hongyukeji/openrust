use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct IndexWebhookIdempotencyRecord;
pub(crate) static MIGRATION: IndexWebhookIdempotencyRecord = IndexWebhookIdempotencyRecord;

impl Migration for IndexWebhookIdempotencyRecord {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_idempotency_keys", |table| {
                    table.index(
                        &["scope_key", "kind", "record_id"],
                        Some("webhook_idempotency_record"),
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
            context
                .get_schema_builder()
                .table("webhook_idempotency_keys", |table| {
                    table.drop_index("webhook_idempotency_record");
                })
                .await
        })
    }
}
