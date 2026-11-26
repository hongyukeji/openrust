use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};

pub struct CreateWebhookDeliveries;
pub(crate) static MIGRATION: CreateWebhookDeliveries = CreateWebhookDeliveries;

impl Migration for CreateWebhookDeliveries {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = context.get_schema_builder();
            schema
                .create("webhook_deliveries", |table| {
                    table.string("id", 64).primary();
                    table.string("scope_key", 64);
                    table.long_text("snapshot");
                    table.string("digest", 64);
                    table.string("state", 16);
                    table.integer("attempts").default(0);
                    table.integer("maximum_tries");
                    table.integer("timeout_seconds");
                    table.big_integer("available_at");
                    table.big_integer("deadline");
                    table.big_integer("lease_until").nullable();
                    table.string("owner", 36).nullable();
                    table.index(
                        &["scope_key", "state", "available_at"],
                        Some("webhook_delivery_ready"),
                    );
                })
                .await?;
            schema
                .create("webhook_delivery_attempts", |table| {
                    table.string("id", 36).primary();
                    table.string("delivery_id", 64);
                    table.integer("number");
                    table.string("outcome", 16);
                    table.integer("http_status").nullable();
                    table.big_integer("started_at");
                    table.big_integer("finished_at").nullable();
                    table.unique(&["delivery_id", "number"], None);
                    table
                        .foreign(&["delivery_id"], None)
                        .references("id")
                        .on("webhook_deliveries")
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
            if let Some(query) = context.query_table("webhook_deliveries")
                && query.exists().await?
            {
                return Err(DatabaseError::InvalidMigration(
                    "Webhook 投递及尝试记录仍在保留期内",
                ));
            }
            let mut schema = context.get_schema_builder();
            schema.drop_if_exists("webhook_delivery_attempts").await?;
            schema.drop_if_exists("webhook_deliveries").await
        })
    }
}
