use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationRevisionControlsTable;
pub(crate) static MIGRATION: CreateModerationRevisionControlsTable =
    CreateModerationRevisionControlsTable;

impl Migration for CreateModerationRevisionControlsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_revision_controls", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.big_integer("withdrawn_at").nullable();
                    table.index(&["scope_key"], Some("moderation_revision_control_scope"));
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("moderation_revision_controls")
                && query.where_not_null("withdrawn_at").exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容修订撤回记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_revision_controls")
                .await
        })
    }
}
