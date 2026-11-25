use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationVideoTombstonesTable;
pub(crate) static MIGRATION: CreateModerationVideoTombstonesTable =
    CreateModerationVideoTombstonesTable;

impl Migration for CreateModerationVideoTombstonesTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_video_tombstones", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.big_integer("expires_at");
                    table.index(
                        &["scope_key", "expires_at"],
                        Some("moderation_video_tombstone_expiry"),
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
            if let Some(query) = context.query_table("moderation_video_tombstones")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核回调关联墓碑"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_video_tombstones")
                .await
        })
    }
}
