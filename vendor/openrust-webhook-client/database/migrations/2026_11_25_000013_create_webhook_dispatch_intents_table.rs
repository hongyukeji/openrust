use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookDispatchIntentsTable;
pub(crate) static MIGRATION: CreateWebhookDispatchIntentsTable = CreateWebhookDispatchIntentsTable;

impl Migration for CreateWebhookDispatchIntentsTable {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("webhook_dispatch_intents", |table| {
                    table.string("call_id", 36).primary();
                    table.string("scope_key", 64);
                    table.text("route");
                    table.big_integer("available_at").default(0);
                    table.string("claim_token", 36).nullable();
                    table.index(
                        &["scope_key", "available_at", "call_id"],
                        Some("webhook_dispatch_ready"),
                    );
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
            if let Some(query) = context.query_table("webhook_dispatch_intents")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 仍有待派发接收记录",
                ));
            }
            context
                .get_schema_builder()
                .drop_if_exists("webhook_dispatch_intents")
                .await
        })
    }
}
