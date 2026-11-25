use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationChecksTable;
pub(crate) static MIGRATION: CreateModerationChecksTable = CreateModerationChecksTable;

impl Migration for CreateModerationChecksTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_checks", |table| {
                    table.string("id", 36).primary();
                    table.string("review_id", 36);
                    table.string("kind", 16);
                    table.text("modalities");
                    table.string("submission", 16);
                    table.big_integer("available_at");
                    table.unique(&["review_id", "kind"], Some("moderation_review_check_kind"));
                    table
                        .foreign(&["review_id"], None)
                        .references("id")
                        .on("moderation_reviews");
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
                .drop_if_exists("moderation_checks")
                .await
        })
    }
}
