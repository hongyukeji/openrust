use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateWebhookProcessingAttemptsTable;
pub(crate) static MIGRATION: CreateWebhookProcessingAttemptsTable =
    CreateWebhookProcessingAttemptsTable;
impl Migration for CreateWebhookProcessingAttemptsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_processing_attempts", |table| {
                    table.string("id", 36).primary();
                    table.string("call_id", 36);
                    table.big_integer("generation");
                    table.big_integer("started_at");
                    table.big_integer("finished_at").nullable();
                    table.string("outcome", 16);
                    table.unique(
                        &["call_id", "generation"],
                        Some("webhook_attempt_generation"),
                    );
                    table.index(&["call_id", "finished_at"], Some("webhook_attempt_open"));
                    table
                        .foreign(&["call_id"], Some("webhook_attempt_receipt"))
                        .references("id")
                        .on("webhook_calls")
                        .cascade_on_delete();
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
                .drop_if_exists("webhook_processing_attempts")
                .await
        })
    }
}
