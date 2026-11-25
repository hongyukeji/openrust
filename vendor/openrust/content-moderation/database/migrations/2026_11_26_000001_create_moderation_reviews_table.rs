use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationReviewsTable;
pub(crate) static MIGRATION: CreateModerationReviewsTable = CreateModerationReviewsTable;

impl Migration for CreateModerationReviewsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_reviews", |table| {
                    table.string("id", 36).primary();
                    table.string("scope_key", 64);
                    table.string("intent_key", 64);
                    table.text("content_id");
                    table.text("content_revision");
                    table.text("profile_revision");
                    table.string("disk", 128);
                    table.text("object_key");
                    table.text("object_version");
                    table.string("state", 16);
                    table.big_integer("created_at");
                    table.big_integer("deadline_at");
                    table.big_integer("version");
                    table.unique(
                        &["scope_key", "intent_key"],
                        Some("moderation_review_intent"),
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
            if let Some(query) = context.query_table("moderation_reviews")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_reviews")
                .await
        })
    }
}
