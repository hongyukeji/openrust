use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookReplaysTable;
pub(crate) static MIGRATION: CreateWebhookReplaysTable = CreateWebhookReplaysTable;

impl Migration for CreateWebhookReplaysTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.get_schema_builder();
            schema
                .table("webhook_calls", |table| {
                    table.integer("replay_generation").default(0);
                })
                .await?;
            schema
                .create("webhook_replays", |table| {
                    table.string("call_id", 36);
                    table.integer("generation");
                    table.string("actor", 128);
                    table.string("reason", 512);
                    table.big_integer("created_at");
                    table.big_integer("expires_at");
                    table.primary(&["call_id", "generation"], None);
                    table
                        .foreign(&["call_id"], None)
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
            if let Some(query) = context.query_table("webhook_replays")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 重放审计仍在保留期内",
                ));
            }
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists("webhook_replays").await?;
            schema
                .table("webhook_calls", |table| {
                    table.drop_column(&["replay_generation"]);
                })
                .await
        })
    }
}
