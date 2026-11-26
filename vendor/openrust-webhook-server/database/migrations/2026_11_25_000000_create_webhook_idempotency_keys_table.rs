use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookIdempotencyKeys;
pub(crate) static MIGRATION: CreateWebhookIdempotencyKeys = CreateWebhookIdempotencyKeys;

impl Migration for CreateWebhookIdempotencyKeys {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_idempotency_keys", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("kind", 16);
                    table.string("record_id", 64);
                    table.index(
                        &["scope_key", "kind", "id"],
                        Some("webhook_idempotency_scope"),
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
            if let Some(query) = context.query_table("webhook_idempotency_keys")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 幂等键映射仍存在"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_idempotency_keys")
                .await
        })
    }
}
