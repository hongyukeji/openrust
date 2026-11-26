use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationLookupHint;
pub(crate) static MIGRATION: AddModerationLookupHint = AddModerationLookupHint;

impl Migration for AddModerationLookupHint {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.string("lookup_hint_id", 36).nullable();
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
                && query.where_not_null("lookup_hint_id").exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "仍有已记录的内容审核查单提示",
                ));
            }
            context
                .get_schema_builder()
                .table("moderation_checks", |table| {
                    table.drop_column(&["lookup_hint_id"]);
                })
                .await
        })
    }
}
