use openrust::{
    contracts::config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext},
    },
};

pub struct AddLifecycleToAiGatewayLiveCalls;
pub(crate) static MIGRATION: AddLifecycleToAiGatewayLiveCalls = AddLifecycleToAiGatewayLiveCalls;

impl Migration for AddLifecycleToAiGatewayLiveCalls {
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
                .table("ai_gateway_live_calls", |table| {
                    table.string("usage_id", 26).nullable();
                    table.big_integer("started_at").nullable();
                    table.boolean("observing").default(false);
                    table.big_integer("handoff_until").default(0);
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
                .table("ai_gateway_live_calls", |table| {
                    table.drop_column(&["usage_id", "started_at", "observing", "handoff_until"]);
                })
                .await
        })
    }
}
