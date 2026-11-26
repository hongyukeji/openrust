use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct AddWebhookCallUpdateTime;
pub(crate) static MIGRATION: AddWebhookCallUpdateTime = AddWebhookCallUpdateTime;

impl Migration for AddWebhookCallUpdateTime {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.big_integer("updated_at").nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .table("webhook_calls", |table| {
                    table.drop_column(&["updated_at"]);
                })
                .await
        })
    }
}
