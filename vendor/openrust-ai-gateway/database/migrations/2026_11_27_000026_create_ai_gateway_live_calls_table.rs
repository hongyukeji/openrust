use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct CreateAiGatewayLiveCallsTable;
pub(crate) static MIGRATION: CreateAiGatewayLiveCallsTable = CreateAiGatewayLiveCallsTable;

impl Migration for CreateAiGatewayLiveCallsTable {
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
                .create("ai_gateway_live_calls", |table| {
                    table.string("id", 26).primary();
                    table.string("owner", 64);
                    table.string("scope", 64);
                    table.string("model", 255);
                    table.string("upstream_id", 128).nullable();
                    table.string("controller", 26).nullable();
                    table.big_integer("controller_until").default(0);
                    table.big_integer("expires_at").index();
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
                .drop_if_exists("ai_gateway_live_calls")
                .await
        })
    }
}
