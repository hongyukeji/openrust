use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookRateBuckets;
pub(crate) static MIGRATION: CreateWebhookRateBuckets = CreateWebhookRateBuckets;

impl Migration for CreateWebhookRateBuckets {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_rate_buckets", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("kind", 16);
                    table.big_integer("used");
                    table.big_integer("reset_at");
                    table.index(&["scope_key", "kind"], Some("webhook_rate_scope_kind"));
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_rate_buckets")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 限额计数仍存在"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_rate_buckets")
                .await
        })
    }
}
