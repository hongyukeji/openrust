use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationReviewDecisionsTable;
pub(crate) static MIGRATION: CreateModerationReviewDecisionsTable =
    CreateModerationReviewDecisionsTable;

impl Migration for CreateModerationReviewDecisionsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_review_decisions", |table| {
                    table.string("scope_key", 64);
                    table.string("review_kind", 16);
                    table.string("review_id", 36);
                    table.big_integer("revision");
                    table.string("decision", 8);
                    table.string("actor", 64);
                    table.text("reason");
                    table.big_integer("decided_at");
                    table.unique(
                        &["scope_key", "review_kind", "review_id", "revision"],
                        Some("moderation_decision_revision"),
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
            if let Some(query) = context.query_table("moderation_review_decisions")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有人工审核裁决记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_review_decisions")
                .await
        })
    }
}
