use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationAccountRateBucketsTable;
pub(crate) static MIGRATION: CreateModerationAccountRateBucketsTable =
    CreateModerationAccountRateBucketsTable;

impl Migration for CreateModerationAccountRateBucketsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("moderation_account_rate_buckets", |table| {
                    table.string("id", 64).primary();
                    table.string("operation", 8);
                    table.big_integer("used");
                    table.big_integer("reset_at");
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("moderation_account_rate_buckets")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核账户速率记录"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_account_rate_buckets")
                .await
        })
    }
}
