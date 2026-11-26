use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationCallbackReference;
pub(crate) static MIGRATION: AddModerationCallbackReference = AddModerationCallbackReference;

impl Migration for AddModerationCallbackReference {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.string("confirmation_callback_id", 36).nullable();
                    table.index(
                        &["remote_task_id", "review_id"],
                        Some("moderation_remote_task"),
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
            if let Some(query) = context.query_table("moderation_checks")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核子检查"));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_index("moderation_remote_task");
                    table.drop_column(&["confirmation_callback_id"]);
                })
                .await
        })
    }
}
