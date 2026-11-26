use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookSubscriptions;
pub(crate) static MIGRATION: CreateWebhookSubscriptions = CreateWebhookSubscriptions;

impl Migration for CreateWebhookSubscriptions {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_subscriptions", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("topic_key", 64);
                    table.string("endpoint_id", 64);
                    table.index(
                        &["scope_key", "topic_key", "endpoint_id"],
                        Some("webhook_subscriptions_select"),
                    );
                    table
                        .foreign(&["endpoint_id"], None)
                        .references("id")
                        .on("webhook_endpoints");
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_subscriptions")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 订阅仍存在"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_subscriptions")
                .await
        })
    }
}
