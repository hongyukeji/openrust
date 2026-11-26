use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationReviewGroupsTable;
pub(crate) static MIGRATION: CreateModerationReviewGroupsTable = CreateModerationReviewGroupsTable;

impl Migration for CreateModerationReviewGroupsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_review_groups", |table| {
                    table.string("id", 36).primary();
                    table.string("scope_key", 64);
                    table.string("intent_key", 64);
                    table.text("member_ids");
                    table.unique(
                        &["scope_key", "intent_key"],
                        Some("moderation_group_intent"),
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
            if let Some(query) = context.query_table("moderation_review_groups")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有复合内容审核记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_review_groups")
                .await
        })
    }
}
