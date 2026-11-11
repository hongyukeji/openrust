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
            let mut schema = connection.getSchemaBuilder();
            schema
                .create("cache", |table| {
                    table.string("key", 255).primary();
                    table.longText("value");
                    table.bigInteger("expiration").index();
                })
                .await?;
            schema
                .create("cache_locks", |table| {
                    table.string("key", 255).primary();
                    table.string("owner", 255);
                    table.bigInteger("expiration").index();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.getSchemaBuilder();
            schema.dropIfExists("cache_locks").await?;
            schema.dropIfExists("cache").await
        })
    }
}
