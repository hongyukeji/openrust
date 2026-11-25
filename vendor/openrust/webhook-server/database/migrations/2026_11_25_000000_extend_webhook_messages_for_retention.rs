use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct ExtendWebhookMessagesForRetention;
pub(crate) static MIGRATION: ExtendWebhookMessagesForRetention = ExtendWebhookMessagesForRetention;

impl Migration for ExtendWebhookMessagesForRetention {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_messages", |table| {
                    table.big_integer("body_pruned_at").nullable();
                    table.index(&["scope_key", "id"], Some("webhook_message_retention"));
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if context
                .get_schema_builder()
                .has_column("webhook_messages", "idempotency_expired_at")
                .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "请先回滚 Webhook 幂等键到期迁移",
                ));
            }
            if let Some(query) = context.query_table("webhook_messages")
                && query.where_not_null("body_pruned_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 共享正文墓碑仍存在",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_messages", |table| {
                    table.drop_index("webhook_message_retention");
                    table.drop_column(&["body_pruned_at"]);
                })
                .await
        })
    }
}
