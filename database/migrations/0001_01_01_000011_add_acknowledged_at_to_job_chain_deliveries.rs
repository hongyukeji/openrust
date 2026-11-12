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
                .table("job_chain_deliveries", |table| {
                    table.bigInteger("acknowledged_at").nullable();
                    table.index(&["acknowledged_at"], None);
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
                .table("job_chain_deliveries", |table| {
                    table.dropIndex("job_chain_deliveries_acknowledged_at_index");
                    table.dropColumn(&["acknowledged_at"]);
                })
                .await
        })
    }
}
