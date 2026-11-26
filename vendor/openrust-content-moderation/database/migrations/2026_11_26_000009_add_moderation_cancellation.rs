use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationCancellation;
pub(crate) static MIGRATION: AddModerationCancellation = AddModerationCancellation;

impl Migration for AddModerationCancellation {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.string("provider_cancellation", 24).nullable();
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
                && query
                    .where_not_null("provider_cancellation")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核取消记录"));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_column(&["provider_cancellation"]);
                })
                .await
        })
    }
}
