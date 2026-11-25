use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct AddWebhookAttachmentCleanupLease;
pub(crate) static MIGRATION: AddWebhookAttachmentCleanupLease = AddWebhookAttachmentCleanupLease;
impl Migration for AddWebhookAttachmentCleanupLease {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_attachment_staging", |table| {
                    table.boolean("detached").default(false);
                    table.string("cleanup_token", 36).nullable();
                    table.big_integer("cleanup_until").nullable();
                })
                .await
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_attachment_staging")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("Webhook 附件清理仍有待办"));
            }
            if let Some(query) = context.query_table("webhook_calls")
                && query
                    .where_not_null("attachment_reference")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 磁盘附件记录仍在使用",
                ));
            }
            context
                .get_schema_builder()
                .table("webhook_attachment_staging", |table| {
                    table.drop_column(&["detached", "cleanup_token", "cleanup_until"]);
                })
                .await
        })
    }
}
