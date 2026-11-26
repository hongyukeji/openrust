use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayUsageTable;
pub(crate) static MIGRATION: CreateAiGatewayUsageTable = CreateAiGatewayUsageTable;

impl Migration for CreateAiGatewayUsageTable {
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
                .create("ai_gateway_usage", |table| {
                    table.string("id", 26).primary();
                    table.string("request_id", 26).index();
                    table.string("key_id", 26).nullable().index();
                    table.text("key_name");
                    table.text("group_name").nullable();
                    table.text("model");
                    table.text("provider");
                    table.text("upstream_model");
                    table.string("endpoint", 64);
                    table.string("upstream_endpoint", 64);
                    table.string("outcome", 48);
                    table.integer("status").nullable();
                    table.big_integer("input_tokens").nullable();
                    table.big_integer("output_tokens").nullable();
                    table.big_integer("cached_input_tokens").nullable();
                    table.big_integer("cache_write_input_tokens").nullable();
                    table.big_integer("reasoning_tokens").nullable();
                    table.big_integer("started_at");
                    table.big_integer("finished_at").nullable();
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
                .drop_if_exists("ai_gateway_usage")
                .await
        })
    }
}
