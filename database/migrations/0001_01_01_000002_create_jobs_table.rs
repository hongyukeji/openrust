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
                .create("jobs", |table| {
                    table.id();
                    table.string("queue", 255).index();
                    table.longText("payload");
                    table.unsignedInteger("attempts");
                    table.unsignedBigInteger("reserved_at").nullable();
                    table.unsignedBigInteger("available_at");
                    table.unsignedBigInteger("created_at");
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { connection.getSchemaBuilder().dropIfExists("jobs").await })
    }
}
