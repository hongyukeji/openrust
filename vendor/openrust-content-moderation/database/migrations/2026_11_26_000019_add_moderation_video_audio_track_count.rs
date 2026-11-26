use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddModerationVideoAudioTrackCount;
pub(crate) static MIGRATION: AddModerationVideoAudioTrackCount = AddModerationVideoAudioTrackCount;

impl Migration for AddModerationVideoAudioTrackCount {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("moderation_reviews", |table| {
                    table.integer("video_audio_track_count").nullable();
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
                && query
                    .where_not_null("video_audio_track_count")
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "仍有已核实视频音轨的内容审核",
                ));
            }
            context
                .get_schema_builder()
                .table("moderation_reviews", |table| {
                    table.drop_column(&["video_audio_track_count"]);
                })
                .await
        })
    }
}
