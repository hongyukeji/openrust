use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationLateNoticesTable;
pub(crate) static MIGRATION: CreateModerationLateNoticesTable = CreateModerationLateNoticesTable;

impl Migration for CreateModerationLateNoticesTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_late_notices", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.string("review_id", 36);
                    table.string("kind", 24);
                    table.string("review_state", 16);
                    table.big_integer("observed_at");
                    table.string("receipt_id", 36).nullable();
                    table.index(
                        &["scope_key", "review_id"],
                        Some("moderation_late_notice_scope"),
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
            if let Some(query) = context.query_table("moderation_late_notices")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核迟到诊断记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_late_notices")
                .await
        })
    }
}
