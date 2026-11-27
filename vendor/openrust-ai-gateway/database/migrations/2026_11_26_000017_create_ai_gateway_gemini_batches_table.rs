use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayGeminiBatchesTable;
pub(crate) static MIGRATION: CreateAiGatewayGeminiBatchesTable = CreateAiGatewayGeminiBatchesTable;

impl Migration for CreateAiGatewayGeminiBatchesTable {
    fn get_connection(&self, config: &dyn Repository) -> DatabaseResult<Option<String>> {
        match config.get("ai_gateway.usage.connection") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(name)) if !name.trim().is_empty() => Ok(Some(name)),
            _ => Err(DatabaseError::InvalidConfiguration(
                "ai_gateway.usage.connection",
            )),
        }
    }

    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("ai_gateway_gemini_batches", |table| {
                    table.string("id", 26).primary();
                    table.string("owner", 64);
                    table.string("scope", 64);
                    table.string("provider", 255);
                    table.string("model", 255);
                    table.string("upstream_model", 255);
                    table.string("upstream_name", 136).nullable();
                    table.string("upstream_identity", 64).nullable().unique();
                    table.string("endpoint", 32);
                    table.integer("item_count");
                    table.big_integer("deleted_at").nullable();
                    table.index(
                        &["owner", "id"],
                        Some("ai_gateway_gemini_batches_owner_idx"),
                    );
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
                .drop_if_exists("ai_gateway_gemini_batches")
                .await
        })
    }
}
