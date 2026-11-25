use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
use openrust::localization::__;

pub struct Definition;

impl Migration for Definition {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.get_schema_builder();
            schema
                .create("cache", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.cache", &Default::default())?);
                    table
                        .string("key", None)
                        .primary()
                        .comment(__("schema.columns.key", &Default::default())?);
                    table
                        .medium_text("value")
                        .comment(__("schema.columns.value", &Default::default())?);
                    table
                        .big_integer("expiration")
                        .index()
                        .comment(__("schema.columns.expiration", &Default::default())?);
                    Ok(())
                })
                .await?;
            schema
                .create("cache_locks", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.cache_locks", &Default::default())?);
                    table
                        .string("key", None)
                        .primary()
                        .comment(__("schema.columns.key", &Default::default())?);
                    table
                        .string("owner", None)
                        .comment(__("schema.columns.owner", &Default::default())?);
                    table
                        .big_integer("expiration")
                        .index()
                        .comment(__("schema.columns.expiration", &Default::default())?);
                    Ok(())
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = connection.get_schema_builder();
            schema.drop_if_exists("cache").await?;
            schema.drop_if_exists("cache_locks").await
        })
    }
}
