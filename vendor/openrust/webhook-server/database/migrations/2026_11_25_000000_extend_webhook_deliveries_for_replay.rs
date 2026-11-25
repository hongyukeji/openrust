use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct ExtendWebhookDeliveriesForReplay;
pub(crate) static MIGRATION: ExtendWebhookDeliveriesForReplay = ExtendWebhookDeliveriesForReplay;

impl Migration for ExtendWebhookDeliveriesForReplay {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_delivery_replays", |table| {
                    table.string("delivery_id", 64).primary();
                    table.string("original_id", 64);
                    table.string("scope_key", 64);
                    table.integer("generation");
                    table.string("actor", 128);
                    table.string("reason", 512);
                    table.string("source_state", 16);
                    table.string("original_credential", 128).nullable();
                    table.string("replay_credential", 128).nullable();
                    table.big_integer("created_at");
                    table.unique(
                        &["original_id", "generation"],
                        Some("webhook_replay_generation"),
                    );
                    table
                        .foreign(&["original_id"], None)
                        .references("id")
                        .on("webhook_deliveries");
                    table
                        .foreign(&["delivery_id"], None)
                        .references("id")
                        .on("webhook_deliveries");
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            if let Some(query) = context.query_table("webhook_delivery_replays")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 重放审计记录仍存在",
                ));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_delivery_replays")
                .await
        })
    }
}
