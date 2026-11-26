use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationVideoDuration;
pub(crate) static MIGRATION: AddModerationVideoDuration = AddModerationVideoDuration;

impl Migration for AddModerationVideoDuration {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_reviews", |table| {
                    table.big_integer("video_duration_ms").nullable();
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
                && query.where_not_null("video_duration_ms").exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "仍有固定素材时长的内容审核",
                ));
            }
            context
                .get_schema_builder()
                .table("moderation_reviews", |table| {
                    table.drop_column(&["video_duration_ms"]);
                })
                .await
        })
    }
}
