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
            connection
                .getSchemaBuilder()
                .create("sms_deliveries", |table| -> DatabaseResult<()> {
                    table.comment(__("schema.tables.sms_deliveries", &Default::default())?);
                    table
                        .string("id", 36)
                        .primary()
                        .comment(__("schema.columns.id", &Default::default())?);
                    table
                        .string("status", 16)
                        .comment(__("schema.columns.sms_status", &Default::default())?);
                    table
                        .text("submission")
                        .nullable()
                        .comment(__("schema.columns.submission", &Default::default())?);
                    table
                        .bigInteger("created_at")
                        .comment(__("schema.columns.created_at_unix", &Default::default())?);
                    table
                        .bigInteger("updated_at")
                        .comment(__("schema.columns.updated_at_unix", &Default::default())?);
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
            connection
                .getSchemaBuilder()
                .dropIfExists("sms_deliveries")
                .await
        })
    }
}
