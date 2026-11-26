use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookMessages;
pub(crate) static MIGRATION: CreateWebhookMessages = CreateWebhookMessages;

impl Migration for CreateWebhookMessages {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_messages", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("topic", 128);
                    table.big_integer("schema_version");
                    table.string("fingerprint", 64);
                    table.long_text("body");
                    table.string("body_digest", 64);
                    table.text("delivery_ids");
                    table.big_integer("created_at");
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
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 消息仍存在"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_messages")
                .await
        })
    }
}
