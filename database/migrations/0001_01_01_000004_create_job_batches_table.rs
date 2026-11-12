use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .getSchemaBuilder()
                .create("job_batches", |table| {
                    table.string("id", 255).primary();
                    table.string("name", 255);
                    table.integer("total_jobs");
                    table.integer("pending_jobs");
                    table.integer("failed_jobs");
                    table.longText("failed_job_ids");
                    table.mediumText("options").nullable();
                    table.bigInteger("cancelled_at").nullable();
                    table.bigInteger("created_at");
                    table.bigInteger("finished_at").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            connection
                .getSchemaBuilder()
                .dropIfExists("job_batches")
                .await
        })
    }
}
