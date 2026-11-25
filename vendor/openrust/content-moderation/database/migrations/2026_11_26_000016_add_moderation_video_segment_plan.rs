use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationVideoSegmentPlan;
pub(crate) static MIGRATION: AddModerationVideoSegmentPlan = AddModerationVideoSegmentPlan;

impl Migration for AddModerationVideoSegmentPlan {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_review_groups", |table| {
                    table.text("segment_plan").nullable();
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
                && query.where_not_null("segment_plan").exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核分段计划"));
            }
            context
                .get_schema_builder()
                .table("moderation_review_groups", |table| {
                    table.drop_column(&["segment_plan"]);
                })
                .await
        })
    }
}
