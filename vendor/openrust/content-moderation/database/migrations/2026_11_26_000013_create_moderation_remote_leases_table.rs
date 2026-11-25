use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateModerationRemoteLeasesTable;
pub(crate) static MIGRATION: CreateModerationRemoteLeasesTable = CreateModerationRemoteLeasesTable;

impl Migration for CreateModerationRemoteLeasesTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let now = clock(context.get_driver_name());
            if let Some(query) = context.query_table("moderation_checks")
                && query
                    .join("moderation_reviews", "moderation_checks.review_id", "=", "moderation_reviews.id")
                    .where_raw(&format!("deadline_at > {now}"), Vec::new())
                    .where_raw(
                        "(submission in ('Submitting', 'Unknown') or (submission = 'Submitted' and (\
                            state in ('Pending', 'Running') or \
                            (state = 'Cancelled' and provider_cancellation is not null and provider_cancellation <> 'acknowledged') or \
                            (state = 'Failed' and coalesce(query_failure, '') not in ('provider_failed', 'incomplete_coverage'))\
                        )))",
                        Vec::new(),
                    )
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration("升级前须处理尚可能运行的旧内容审核任务"));
            }
            context
                .get_schema_builder()
                .create("moderation_remote_leases", |table| {
                    table.string("check_id", 36).primary();
                    table.string("account_key", 64);
                    table.big_integer("expires_at");
                    table.index(
                        &["account_key", "expires_at"],
                        Some("moderation_remote_lease_expiry"),
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
            let now = clock(context.get_driver_name());
            if let Some(query) = context.query_table("moderation_remote_leases")
                && query
                    .where_raw(&format!("expires_at > {now}"), Vec::new())
                    .exists()
                    .await?
            {
                return Err(DatabaseError::InvalidMigration("仍有内容审核远端在途租约"));
            }
            context
                .get_schema_builder()
                .drop_if_exists("moderation_remote_leases")
                .await
        })
    }
}

fn clock(driver: &str) -> &'static str {
    match driver {
        "sqlite" => "unixepoch()",
        "pgsql" => "floor(extract(epoch from statement_timestamp()))::bigint",
        "sqlsrv" => "datediff_big(second, '19700101', sysutcdatetime())",
        _ => "unix_timestamp()",
    }
}
