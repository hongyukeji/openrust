use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
use openrust::translation::__;

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.getSchemaBuilder();
            schema
                .create("jobs", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.jobs", &Default::default())?);
                    table
                        .id()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .string("queue", None)
                        .index()
                        .comment(__("schema.columns.queue", &Default::default())?);
                    table
                        .longText("payload")
                        .comment(__("schema.columns.payload", &Default::default())?);
                    table
                        .unsignedSmallInteger("attempts")
                        .comment(__("schema.columns.attempts", &Default::default())?);
                    table
                        .unsignedBigInteger("reserved_at")
                        .nullable()
                        .comment(__("schema.columns.reserved_at", &Default::default())?);
                    table
                        .unsignedBigInteger("available_at")
                        .comment(__("schema.columns.available_at", &Default::default())?);
                    table
                        .unsignedBigInteger("created_at")
                        .comment(__("schema.columns.created_at_unix", &Default::default())?);
                    Ok(())
                })
                .await?;
            schema
                .create("job_batches", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.job_batches", &Default::default())?);
                    table
                        .string("id", None)
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .string("name", None)
                        .comment(__("schema.columns.batch_name", &Default::default())?);
                    table
                        .integer("total_jobs")
                        .comment(__("schema.columns.total_jobs", &Default::default())?);
                    table
                        .integer("pending_jobs")
                        .comment(__("schema.columns.pending_jobs", &Default::default())?);
                    table
                        .integer("failed_jobs")
                        .comment(__("schema.columns.failed_jobs", &Default::default())?);
                    table
                        .text("failed_job_ids")
                        .comment(__("schema.columns.failed_job_ids", &Default::default())?);
                    table
                        .text("options")
                        .nullable()
                        .comment(__("schema.columns.options", &Default::default())?);
                    table
                        .integer("cancelled_at")
                        .nullable()
                        .comment(__("schema.columns.cancelled_at", &Default::default())?);
                    table
                        .integer("created_at")
                        .comment(__("schema.columns.created_at_unix", &Default::default())?);
                    table
                        .integer("finished_at")
                        .nullable()
                        .comment(__("schema.columns.finished_at", &Default::default())?);
                    Ok(())
                })
                .await?;
            schema
                .create("failed_jobs", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.failed_jobs", &Default::default())?);
                    table
                        .id()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .string("uuid", None)
                        .unique()
                        .comment(__("schema.columns.uuid", &Default::default())?);
                    table
                        .text("connection")
                        .comment(__("schema.columns.connection", &Default::default())?);
                    table
                        .text("queue")
                        .comment(__("schema.columns.queue", &Default::default())?);
                    table
                        .longText("payload")
                        .comment(__("schema.columns.payload", &Default::default())?);
                    table
                        .longText("exception")
                        .comment(__("schema.columns.exception", &Default::default())?);
                    table
                        .timestamp("failed_at", None)
                        .useCurrent()
                        .comment(__("schema.columns.failed_at", &Default::default())?);
                    Ok(())
                })
                .await?;
            Ok(())
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.getSchemaBuilder();
            schema.dropIfExists("jobs").await?;
            schema.dropIfExists("job_batches").await?;
            schema.dropIfExists("failed_jobs").await
        })
    }
}
