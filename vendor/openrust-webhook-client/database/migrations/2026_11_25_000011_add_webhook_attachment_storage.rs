use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookAttachmentStorage;
pub(crate) static MIGRATION: AddWebhookAttachmentStorage = AddWebhookAttachmentStorage;

impl Migration for AddWebhookAttachmentStorage {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.get_schema_builder();
            schema
                .table("webhook_calls", |table| {
                    table.text("attachment_reference").nullable();
                })
                .await?;
            schema
                .create("webhook_attachment_staging", |table| {
                    table.string("id", 36).primary();
                    table.string("scope_key", 64);
                    table.string("disk", 128);
                    table.big_integer("created_at");
                    table.index(
                        &["scope_key", "created_at"],
                        Some("webhook_attachment_stage_age"),
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
            if let Some(receipts) = context.query_table("webhook_calls")
                && receipts
                    .where_not_null("attachment_reference")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 磁盘附件记录仍在使用",
                ));
            }
            if let Some(staging) = context.query_table("webhook_attachment_staging")
                && staging.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 附件暂存登记仍在使用",
                ));
            }
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists("webhook_attachment_staging").await?;
            schema
                .table("webhook_calls", |table| {
                    table.drop_column(&["attachment_reference"]);
                })
                .await
        })
    }
}
