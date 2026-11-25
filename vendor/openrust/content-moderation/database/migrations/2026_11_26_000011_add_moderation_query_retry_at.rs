use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationQueryRetryAt;
pub(crate) static MIGRATION: AddModerationQueryRetryAt = AddModerationQueryRetryAt;

impl Migration for AddModerationQueryRetryAt {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.big_integer("provider_retry_at").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("moderation_checks")
                && query.where_not_null("provider_retry_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有供应商查单等待记录"));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_column(&["provider_retry_at"]);
                })
                .await
        })
    }
}
