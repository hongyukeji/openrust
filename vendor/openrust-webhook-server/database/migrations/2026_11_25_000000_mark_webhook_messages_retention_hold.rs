use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookMessagesRetentionHold;
pub(crate) static MIGRATION: AddWebhookMessagesRetentionHold = AddWebhookMessagesRetentionHold;

impl Migration for AddWebhookMessagesRetentionHold {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_messages", |table| {
                    table.big_integer("retention_hold_at").nullable();
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
                && query.where_not_null("retention_hold_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 记录仍处于保留状态",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_messages", |table| {
                    table.drop_column(&["retention_hold_at"]);
                })
                .await
        })
    }
}
