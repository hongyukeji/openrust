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
                .create("job_batches_additions", |table| {
                    table.string("id", 36).primary();
                    table.string("batch_id", 255);
                    table.string("connection", 255);
                    table.string("queue", 255);
                    table.longText("payloads");
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
                .dropIfExists("job_batches_additions")
                .await
        })
    }
}
