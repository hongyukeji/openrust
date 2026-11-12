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
                .create("job_batches_jobs", |table| {
                    table.string("batch_id", 255);
                    table.string("job_id", 36);
                    table.string("delivery_id", 36);
                    table.boolean("succeeded");
                    table.primary(&["batch_id", "job_id", "delivery_id"], None);
                    table
                        .foreign(&["batch_id"], None)
                        .references("id")
                        .on("job_batches")
                        .cascadeOnDelete();
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
                .dropIfExists("job_batches_jobs")
                .await
        })
    }
}
