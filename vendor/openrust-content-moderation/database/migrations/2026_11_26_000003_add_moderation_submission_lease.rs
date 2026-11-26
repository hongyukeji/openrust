use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationSubmissionLease;
pub(crate) static MIGRATION: AddModerationSubmissionLease = AddModerationSubmissionLease;

impl Migration for AddModerationSubmissionLease {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.big_integer("submit_lease_until").nullable();
                    table.string("remote_task_id", 256).nullable();
                    table.string("submit_failure", 32).nullable();
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
                return Err(DatabaseError::InvalidMigration("点播检查已有送审历史"));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_column(&["submit_lease_until", "remote_task_id", "submit_failure"]);
                })
                .await
        })
    }
}
