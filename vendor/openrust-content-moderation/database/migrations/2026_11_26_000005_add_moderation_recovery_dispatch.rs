use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationRecoveryDispatch;
pub(crate) static MIGRATION: AddModerationRecoveryDispatch = AddModerationRecoveryDispatch;

impl Migration for AddModerationRecoveryDispatch {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.big_integer("dispatch_lease_until").nullable();
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
                    table.drop_column(&["dispatch_lease_until"]);
                })
                .await
        })
    }
}
