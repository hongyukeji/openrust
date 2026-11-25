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
            connection
                .get_schema_builder()
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
                        .big_integer("created_at")
                        .comment(__("schema.columns.created_at_unix", &Default::default())?);
                    table
                        .big_integer("updated_at")
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
                .get_schema_builder()
                .drop_if_exists("sms_deliveries")
                .await
        })
    }
}
